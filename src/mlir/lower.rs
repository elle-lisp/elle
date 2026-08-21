//! Lower GPU-eligible LirFunction to MLIR.
//!
//! Produces an MLIR module using the arith, func, cf, and memref dialects.
//! Only handles the GPU-safe instruction subset — no heap allocation,
//! closures, function calls, or signal emission.
//!
//! Local slots use `memref.alloca` for correct cross-block semantics
//! (StoreLocal in one block, LoadLocal in another).

use crate::lir::{
    BinOp, CmpOp, ConvOp, Label, LirConst, LirFunction, LirInstr, Reg, Terminator, UnaryOp,
};
use melior::dialect::arith::{CmpfPredicate, CmpiPredicate};
use melior::dialect::{arith, cf, func, memref, DialectRegistry};
use melior::ir::attribute::{FloatAttribute, IntegerAttribute, StringAttribute, TypeAttribute};
use melior::ir::operation::OperationLike;
use melior::ir::r#type::{FunctionType, IntegerType, MemRefType};
use melior::ir::{Block, BlockLike, Location, Module, Region, RegionLike, Type, Value};
use melior::Context;
use std::collections::HashMap;

mod module;
pub use module::*;

/// Scalar type tag for MLIR register tracking.
///
/// Tracks whether an MLIR SSA value holds an `i64` (integer) or `f64`
/// (float). Used during lowering to dispatch between integer and float
/// MLIR ops, and by the caller to rebox the result correctly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarType {
    Int,
    Float,
    Bool,
}

impl ScalarType {
    /// True if this type is represented as f64 at the MLIR level.
    pub fn is_float(self) -> bool {
        self == ScalarType::Float
    }
}

/// Index into the environment argument list (`[captures..., params...]`).
///
/// Kept distinct from [`Reg`] on purpose: LIR register numbers can collide
/// with env indices, so `lower_to_module` holds env values in their own map.
/// Making the key a separate type means a register can never be used to look
/// up an env value (or vice-versa) — the invariant is enforced by the
/// compiler instead of by a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EnvIndex(u32);

impl EnvIndex {
    fn new(n: u32) -> Self {
        EnvIndex(n)
    }
}

/// Identifier of a local memref slot.
///
/// Distinct from [`Reg`] and [`EnvIndex`] so a slot number can't be used to
/// index the register or env maps. Shared with the SPIR-V emitter
/// ([`super::spirv`]), which keys its local-slot maps by the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SlotId(u32);

impl SlotId {
    pub(super) fn new(n: u32) -> Self {
        SlotId(n)
    }
}

/// Create an MLIR context with all dialects registered.
pub fn create_context() -> Context {
    let context = Context::new();
    let registry = DialectRegistry::new();
    melior::utility::register_all_dialects(&registry);
    context.append_dialect_registry(&registry);
    context.load_all_available_dialects();
    context
}

/// Pre-scan for cross-block mixed-type local slots.
///
/// Walks all blocks and checks that each local slot is only stored with
/// one scalar type across different blocks. Within-block sequential
/// reassignment (e.g. `var s = 0; s = 1.5`) is allowed.
///
/// Called before `lower_to_module` to avoid partially constructing MLIR
/// ops before discovering the error.
pub fn check_slot_types(
    lir: &LirFunction,
    num_captures: u16,
    capture_types: u64,
    param_types: u64,
) -> Result<(), String> {
    // For each slot, track (type, block_idx) of the last store per block.
    // Keyed by SlotId so a slot number can never index the register map.
    let mut slot_block_types: HashMap<SlotId, (ScalarType, usize)> = HashMap::new();
    // Inferred type per local slot, read back by LoadLocal. Kept separate
    // from `reg_types`: registers and slots are independent id spaces that
    // overlap numerically, so sharing one map lets a slot store clobber the
    // register with the same number (and vice-versa).
    let mut slot_types: HashMap<SlotId, ScalarType> = HashMap::new();
    // Simple type inference: track register types from constants and ops.
    let mut reg_types: HashMap<Reg, ScalarType> = HashMap::new();
    let num_params = lir.arity.fixed_params();

    // Argument (capture/param) scalar types, indexed in env-layout order
    // [captures..., params...]. EnvIndex keeps this keyspace distinct from
    // registers and slots. This is the single source of truth for argument
    // types — `LoadCapture` looks them up here by env index.
    let scalar_of = |mask: u64, bit: usize| {
        if mask & (1u64 << bit) != 0 {
            ScalarType::Float
        } else {
            ScalarType::Int
        }
    };
    let mut env_types: HashMap<EnvIndex, ScalarType> = HashMap::new();
    for i in 0..num_captures as usize {
        env_types.insert(EnvIndex::new(i as u32), scalar_of(capture_types, i));
    }
    for i in 0..num_params {
        env_types.insert(
            EnvIndex::new((num_captures as usize + i) as u32),
            scalar_of(param_types, i),
        );
    }

    for (block_idx, block) in lir.blocks.iter().enumerate() {
        for si in &block.instructions {
            match &si.instr {
                LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                    // Env layout [captures..., params...]; types seeded above.
                    let t = env_types
                        .get(&EnvIndex::new(*index as u32))
                        .copied()
                        .unwrap_or(ScalarType::Int);
                    reg_types.insert(*dst, t);
                }
                LirInstr::Const { dst, value } => {
                    let t = match value {
                        LirConst::Float(_) => ScalarType::Float,
                        LirConst::Bool(_) => ScalarType::Bool,
                        _ => ScalarType::Int,
                    };
                    reg_types.insert(*dst, t);
                }
                LirInstr::BinOp { dst, lhs, rhs, op } => {
                    let lt = reg_types.get(lhs).copied().unwrap_or(ScalarType::Int);
                    let rt = reg_types.get(rhs).copied().unwrap_or(ScalarType::Int);
                    let is_bitwise = matches!(
                        op,
                        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                    );
                    let t = if is_bitwise {
                        ScalarType::Int
                    } else if lt.is_float() || rt.is_float() {
                        ScalarType::Float
                    } else {
                        ScalarType::Int
                    };
                    reg_types.insert(*dst, t);
                }
                LirInstr::Compare { dst, .. } => {
                    reg_types.insert(*dst, ScalarType::Bool);
                }
                LirInstr::UnaryOp { dst, op, src } => {
                    let st = reg_types.get(src).copied().unwrap_or(ScalarType::Int);
                    let t = match op {
                        UnaryOp::Neg => st,
                        UnaryOp::Not | UnaryOp::BitNot => ScalarType::Int,
                    };
                    reg_types.insert(*dst, t);
                }
                LirInstr::Convert { dst, op, .. } => {
                    let t = match op {
                        crate::lir::ConvOp::IntToFloat => ScalarType::Float,
                        crate::lir::ConvOp::FloatToInt => ScalarType::Int,
                    };
                    reg_types.insert(*dst, t);
                }
                LirInstr::StoreLocal { slot, src } => {
                    let src_type = reg_types.get(src).copied().unwrap_or(ScalarType::Int);
                    let slot_id = SlotId::new(*slot as u32);
                    if let Some((prev_type, prev_block)) = slot_block_types.get(&slot_id) {
                        // Float vs non-Float is a real conflict (different bit
                        // representation). Bool vs Int are both i64 — no conflict.
                        if prev_type.is_float() != src_type.is_float() && *prev_block != block_idx {
                            return Err(format!(
                                "mixed-type local slot {}: {:?} in block {}, {:?} in block {}",
                                slot, prev_type, prev_block, src_type, block_idx
                            ));
                        }
                    }
                    slot_block_types.insert(slot_id, (src_type, block_idx));
                    slot_types.insert(slot_id, src_type);
                }
                LirInstr::LoadLocal { dst, slot } => {
                    let t = slot_types
                        .get(&SlotId::new(*slot as u32))
                        .copied()
                        .unwrap_or(ScalarType::Int);
                    reg_types.insert(*dst, t);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Lower a GPU-eligible LirFunction to MLIR text (for debugging/testing).
pub fn lower_to_mlir(lir: &LirFunction) -> Result<String, String> {
    let context = create_context();
    let (module, _) = lower_to_module(&context, lir, 0, 0, 0)?;
    Ok(module.as_operation().to_string())
}
