//! Lower GPU-eligible LirFunction to MLIR.
//!
//! Produces an MLIR module using the arith, func, cf, and memref dialects.
//! Only handles the GPU-safe instruction subset — no heap allocation,
//! closures, function calls, or signal emission.
//!
//! Local slots use `memref.alloca` for correct cross-block semantics
//! (StoreLocal in one block, LoadLocal in another).

use crate::lir::{BinOp, CmpOp, ConvOp, LirConst, LirFunction, LirInstr, Terminator, UnaryOp};
use melior::dialect::arith::{CmpfPredicate, CmpiPredicate};
use melior::dialect::{arith, cf, func, memref, DialectRegistry};
use melior::ir::attribute::{FloatAttribute, IntegerAttribute, StringAttribute, TypeAttribute};
use melior::ir::operation::OperationLike;
use melior::ir::r#type::{FunctionType, IntegerType, MemRefType};
use melior::ir::{Block, BlockLike, Location, Module, Region, RegionLike, Type, Value};
use melior::Context;
use std::collections::HashMap;

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
    let mut slot_block_types: HashMap<u32, (ScalarType, usize)> = HashMap::new();
    // Simple type inference: track register types from constants and ops.
    let mut reg_types: HashMap<u32, ScalarType> = HashMap::new();
    let num_params = lir.arity.fixed_params();

    // Seed types for captures (indices 0..num_captures) and params
    // (indices num_captures..num_captures+num_params) — these are
    // the MLIR function arguments in order.
    for i in 0..num_captures as usize {
        let t = if capture_types & (1u64 << i) != 0 {
            ScalarType::Float
        } else {
            ScalarType::Int
        };
        reg_types.insert(i as u32, t);
    }
    for i in 0..num_params {
        let t = if param_types & (1u64 << i) != 0 {
            ScalarType::Float
        } else {
            ScalarType::Int
        };
        reg_types.insert((num_captures as usize + i) as u32, t);
    }

    for (block_idx, block) in lir.blocks.iter().enumerate() {
        for si in &block.instructions {
            match &si.instr {
                LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                    // Env layout: [captures..., params...].
                    // At MLIR level these are contiguous function arguments.
                    let idx = *index as usize;
                    let t = if idx < num_captures as usize {
                        // Capture — type from capture_types bitmask
                        if capture_types & (1u64 << idx) != 0 {
                            ScalarType::Float
                        } else {
                            ScalarType::Int
                        }
                    } else {
                        // Parameter — type from param_types bitmask
                        let param_idx = idx - num_captures as usize;
                        if param_types & (1u64 << param_idx) != 0 {
                            ScalarType::Float
                        } else {
                            ScalarType::Int
                        }
                    };
                    reg_types.insert(dst.0, t);
                }
                LirInstr::Const { dst, value } => {
                    let t = match value {
                        LirConst::Float(_) => ScalarType::Float,
                        LirConst::Bool(_) => ScalarType::Bool,
                        _ => ScalarType::Int,
                    };
                    reg_types.insert(dst.0, t);
                }
                LirInstr::BinOp { dst, lhs, rhs, op } => {
                    let lt = reg_types.get(&lhs.0).copied().unwrap_or(ScalarType::Int);
                    let rt = reg_types.get(&rhs.0).copied().unwrap_or(ScalarType::Int);
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
                    reg_types.insert(dst.0, t);
                }
                LirInstr::Compare { dst, .. } => {
                    reg_types.insert(dst.0, ScalarType::Bool);
                }
                LirInstr::UnaryOp { dst, op, src } => {
                    let st = reg_types.get(&src.0).copied().unwrap_or(ScalarType::Int);
                    let t = match op {
                        UnaryOp::Neg => st,
                        UnaryOp::Not | UnaryOp::BitNot => ScalarType::Int,
                    };
                    reg_types.insert(dst.0, t);
                }
                LirInstr::Convert { dst, op, .. } => {
                    let t = match op {
                        crate::lir::ConvOp::IntToFloat => ScalarType::Float,
                        crate::lir::ConvOp::FloatToInt => ScalarType::Int,
                    };
                    reg_types.insert(dst.0, t);
                }
                LirInstr::StoreLocal { slot, src } => {
                    let src_type = reg_types.get(&src.0).copied().unwrap_or(ScalarType::Int);
                    let slot_key = *slot as u32;
                    if let Some((prev_type, prev_block)) = slot_block_types.get(&slot_key) {
                        // Float vs non-Float is a real conflict (different bit
                        // representation). Bool vs Int are both i64 — no conflict.
                        if prev_type.is_float() != src_type.is_float() && *prev_block != block_idx {
                            return Err(format!(
                                "mixed-type local slot {}: {:?} in block {}, {:?} in block {}",
                                slot, prev_type, prev_block, src_type, block_idx
                            ));
                        }
                    }
                    slot_block_types.insert(slot_key, (src_type, block_idx));
                    reg_types.insert(slot_key, src_type);
                }
                LirInstr::LoadLocal { dst, slot } => {
                    let t = reg_types
                        .get(&(*slot as u32))
                        .copied()
                        .unwrap_or(ScalarType::Int);
                    reg_types.insert(dst.0, t);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Lower a GPU-eligible LirFunction into an MLIR module.
///
/// The module contains a single `func.func` with `llvm.emit_c_interface`
/// so the execution engine can call it via C calling convention.
///
/// Local slots are allocated with `memref.alloca` in the entry block
/// for correct cross-block semantics (phi-like patterns via memory).
pub fn lower_to_module<'c>(
    context: &'c Context,
    lir: &LirFunction,
    num_captures: u16,
    capture_types: u64,
    param_types: u64,
) -> Result<(Module<'c>, ScalarType), String> {
    // Pre-scan for cross-block mixed-type slots before building any MLIR ops.
    check_slot_types(lir, num_captures, capture_types, param_types)?;

    let location = Location::unknown(context);
    let module = Module::new(location);

    let i64_type: Type = IntegerType::new(context, 64).into();
    let f64_type: Type = Type::float64(context);
    let num_params = lir.arity.fixed_params();
    let total_args = num_captures as usize + num_params;

    let mlir_param_types: Vec<Type> = (0..total_args).map(|_| i64_type).collect();
    let func_type = FunctionType::new(context, &mlir_param_types, &[i64_type]);
    let func_name = lir.name.as_deref().unwrap_or("gpu_kernel");

    let region = Region::new();

    // Map LIR labels to block indices
    let mut label_to_idx: HashMap<u32, usize> = HashMap::new();
    let mut blocks: Vec<Block> = Vec::new();

    for (i, lir_block) in lir.blocks.iter().enumerate() {
        let block = if i == 0 {
            Block::new(
                &mlir_param_types
                    .iter()
                    .map(|t| (*t, location))
                    .collect::<Vec<_>>(),
            )
        } else {
            Block::new(&[])
        };
        label_to_idx.insert(lir_block.label.0, i);
        blocks.push(block);
    }

    // SSA register map: LIR Reg → MLIR Value (for within-block SSA values)
    let mut regs: HashMap<u32, Value> = HashMap::new();
    // Type map: LIR Reg → ScalarType (Int or Float)
    let mut types: HashMap<u32, ScalarType> = HashMap::new();
    // Environment values: env index → (MLIR Value, ScalarType).
    // Separate from `regs` so LoadCapture lookups are never clobbered by
    // destination register writes (LIR reg indices can collide with env indices).
    let mut env_vals: HashMap<u32, (Value, ScalarType)> = HashMap::new();
    // Local slot types: slot index → ScalarType
    let mut slot_types: HashMap<u32, ScalarType> = HashMap::new();
    // Return type: determined from Return terminators
    let mut return_type: Option<ScalarType> = None;

    // Allocate memref slots for locals in the entry block.
    // Local slots handle cross-block value passing (phi patterns).
    let scalar_memref = MemRefType::new(i64_type, &[], None, None);
    let num_locals = lir.num_locals as u32;
    let mut local_slots: HashMap<u32, Value> = HashMap::new();

    if !blocks.is_empty() {
        let entry = &blocks[0];

        // Pre-populate env_vals with entry block arguments.
        // MLIR signature: [captures..., params...], all i64.
        // Captures marked as Float in capture_types get bitcast i64→f64.
        for i in 0..num_captures as usize {
            let raw: Value = entry.argument(i).unwrap().into();
            if capture_types & (1u64 << i) != 0 {
                let bc = entry.append_operation(arith::bitcast(raw, f64_type, location));
                env_vals.insert(i as u32, (bc.result(0).unwrap().into(), ScalarType::Float));
            } else {
                env_vals.insert(i as u32, (raw, ScalarType::Int));
            }
        }
        // Params follow captures in the MLIR argument list.
        for i in 0..num_params {
            let arg_idx = num_captures as usize + i;
            let raw: Value = entry.argument(arg_idx).unwrap().into();
            if param_types & (1u64 << i) != 0 {
                let bc = entry.append_operation(arith::bitcast(raw, f64_type, location));
                env_vals.insert(
                    arg_idx as u32,
                    (bc.result(0).unwrap().into(), ScalarType::Float),
                );
            } else {
                env_vals.insert(arg_idx as u32, (raw, ScalarType::Int));
            }
        }

        // Allocate a memref<i64> for each local slot
        for slot in 0..num_locals {
            let alloca_op = entry.append_operation(memref::alloca(
                context,
                scalar_memref,
                &[],
                &[],
                None,
                location,
            ));
            local_slots.insert(slot, alloca_op.result(0).unwrap().into());
        }
    }

    // Lower instructions
    for (block_idx, lir_block) in lir.blocks.iter().enumerate() {
        let block = &blocks[block_idx];

        for si in &lir_block.instructions {
            match &si.instr {
                LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                    // Env layout: [captures..., params...].
                    // MLIR arguments mirror this layout, so index maps directly
                    // to the MLIR block argument index.
                    let idx = *index as usize;
                    if idx < total_args {
                        // Use env_vals (never clobbered by dst writes) to look
                        // up the (possibly bitcast) MLIR value and its type.
                        if let Some(&(val, t)) = env_vals.get(&(idx as u32)) {
                            regs.insert(dst.0, val);
                            types.insert(dst.0, t);
                        } else {
                            // Fallback: shouldn't happen if env_vals was populated
                            regs.insert(dst.0, blocks[0].argument(idx).unwrap().into());
                            types.insert(dst.0, ScalarType::Int);
                        }
                    }
                }
                LirInstr::Const { dst, value } => match value {
                    LirConst::Float(f) => {
                        let op = arith::constant(
                            context,
                            FloatAttribute::new(context, f64_type, *f).into(),
                            location,
                        );
                        let op_ref = block.append_operation(op);
                        regs.insert(dst.0, op_ref.result(0).unwrap().into());
                        types.insert(dst.0, ScalarType::Float);
                    }
                    _ => {
                        let (n, scalar_type) = match value {
                            LirConst::Int(n) => (*n, ScalarType::Int),
                            LirConst::Bool(b) => (i64::from(*b), ScalarType::Bool),
                            LirConst::Nil => (0i64, ScalarType::Int),
                            _ => return Err(format!("unsupported constant: {:?}", value)),
                        };
                        let op = arith::constant(
                            context,
                            IntegerAttribute::new(i64_type, n).into(),
                            location,
                        );
                        let op_ref = block.append_operation(op);
                        regs.insert(dst.0, op_ref.result(0).unwrap().into());
                        types.insert(dst.0, scalar_type);
                    }
                },
                LirInstr::BinOp { dst, op, lhs, rhs } => {
                    let lv = *regs
                        .get(&lhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", lhs.0))?;
                    let rv = *regs
                        .get(&rhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", rhs.0))?;
                    let lt = types.get(&lhs.0).copied().unwrap_or(ScalarType::Int);
                    let rt = types.get(&rhs.0).copied().unwrap_or(ScalarType::Int);

                    let is_bitwise = matches!(
                        op,
                        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                    );
                    if is_bitwise && (lt == ScalarType::Float || rt == ScalarType::Float) {
                        return Err("bitwise ops on float operands not supported".to_string());
                    }

                    // Promote mixed operands: int → float via sitofp
                    let (eff_lv, eff_rv, result_type) = match (lt, rt) {
                        (ScalarType::Int, ScalarType::Int) => (lv, rv, ScalarType::Int),
                        (ScalarType::Float, ScalarType::Float) => (lv, rv, ScalarType::Float),
                        (ScalarType::Int, ScalarType::Float) => {
                            let p = block.append_operation(arith::sitofp(lv, f64_type, location));
                            (p.result(0).unwrap().into(), rv, ScalarType::Float)
                        }
                        (ScalarType::Float, ScalarType::Int) => {
                            let p = block.append_operation(arith::sitofp(rv, f64_type, location));
                            (lv, p.result(0).unwrap().into(), ScalarType::Float)
                        }
                        // Bool operands: treat as Int (0/1)
                        (ScalarType::Bool, ScalarType::Bool) => (lv, rv, ScalarType::Int),
                        (ScalarType::Bool, other) | (other, ScalarType::Bool) => {
                            // Promote bool to the other operand's type
                            (lv, rv, other)
                        }
                    };

                    let mlir_op = if result_type == ScalarType::Float {
                        match op {
                            BinOp::Add => arith::addf(eff_lv, eff_rv, location),
                            BinOp::Sub => arith::subf(eff_lv, eff_rv, location),
                            BinOp::Mul => arith::mulf(eff_lv, eff_rv, location),
                            BinOp::Div => arith::divf(eff_lv, eff_rv, location),
                            BinOp::Rem => arith::remf(eff_lv, eff_rv, location),
                            _ => unreachable!("bitwise on float rejected above"),
                        }
                    } else {
                        match op {
                            BinOp::Add => arith::addi(lv, rv, location),
                            BinOp::Sub => arith::subi(lv, rv, location),
                            BinOp::Mul => arith::muli(lv, rv, location),
                            BinOp::Div => arith::divsi(lv, rv, location),
                            BinOp::Rem => arith::remsi(lv, rv, location),
                            BinOp::BitAnd => arith::andi(lv, rv, location),
                            BinOp::BitOr => arith::ori(lv, rv, location),
                            BinOp::BitXor => arith::xori(lv, rv, location),
                            BinOp::Shl => arith::shli(lv, rv, location),
                            BinOp::Shr => arith::shrsi(lv, rv, location),
                        }
                    };
                    let op_ref = block.append_operation(mlir_op);
                    regs.insert(dst.0, op_ref.result(0).unwrap().into());
                    types.insert(dst.0, result_type);
                }
                LirInstr::Compare { dst, op, lhs, rhs } => {
                    let lv = *regs
                        .get(&lhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", lhs.0))?;
                    let rv = *regs
                        .get(&rhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", rhs.0))?;
                    let lt = types.get(&lhs.0).copied().unwrap_or(ScalarType::Int);
                    let rt = types.get(&rhs.0).copied().unwrap_or(ScalarType::Int);
                    let use_float = lt == ScalarType::Float || rt == ScalarType::Float;

                    let op_ref = if use_float {
                        // Promote mixed operands for float comparison
                        let (eff_lv, eff_rv) = match (lt, rt) {
                            (ScalarType::Float, ScalarType::Float) => (lv, rv),
                            (ScalarType::Int, ScalarType::Float) => {
                                let p =
                                    block.append_operation(arith::sitofp(lv, f64_type, location));
                                (p.result(0).unwrap().into(), rv)
                            }
                            (ScalarType::Float, ScalarType::Int) => {
                                let p =
                                    block.append_operation(arith::sitofp(rv, f64_type, location));
                                (lv, p.result(0).unwrap().into())
                            }
                            _ => unreachable!(),
                        };
                        let pred = match op {
                            CmpOp::Eq => CmpfPredicate::Oeq,
                            CmpOp::Ne => CmpfPredicate::One,
                            CmpOp::Lt => CmpfPredicate::Olt,
                            CmpOp::Le => CmpfPredicate::Ole,
                            CmpOp::Gt => CmpfPredicate::Ogt,
                            CmpOp::Ge => CmpfPredicate::Oge,
                        };
                        block.append_operation(arith::cmpf(context, pred, eff_lv, eff_rv, location))
                    } else {
                        let pred = match op {
                            CmpOp::Eq => CmpiPredicate::Eq,
                            CmpOp::Ne => CmpiPredicate::Ne,
                            CmpOp::Lt => CmpiPredicate::Slt,
                            CmpOp::Le => CmpiPredicate::Sle,
                            CmpOp::Gt => CmpiPredicate::Sgt,
                            CmpOp::Ge => CmpiPredicate::Sge,
                        };
                        block.append_operation(arith::cmpi(context, pred, lv, rv, location))
                    };
                    // cmpi/cmpf returns i1; extend to i64 for consistency
                    let i1_val: Value = op_ref.result(0).unwrap().into();
                    let ext_ref = block.append_operation(arith::extui(i1_val, i64_type, location));
                    regs.insert(dst.0, ext_ref.result(0).unwrap().into());
                    types.insert(dst.0, ScalarType::Bool);
                }
                LirInstr::UnaryOp { dst, op, src } => {
                    let sv = *regs
                        .get(&src.0)
                        .ok_or_else(|| format!("undefined reg r{}", src.0))?;
                    let src_type = types.get(&src.0).copied().unwrap_or(ScalarType::Int);
                    let (result, result_type) = match op {
                        UnaryOp::Neg => {
                            if src_type == ScalarType::Float {
                                let neg = block.append_operation(arith::negf(sv, location));
                                (neg.result(0).unwrap().into(), ScalarType::Float)
                            } else {
                                let zero = block.append_operation(arith::constant(
                                    context,
                                    IntegerAttribute::new(i64_type, 0).into(),
                                    location,
                                ));
                                let zero_val: Value = zero.result(0).unwrap().into();
                                let sub =
                                    block.append_operation(arith::subi(zero_val, sv, location));
                                (sub.result(0).unwrap().into(), ScalarType::Int)
                            }
                        }
                        UnaryOp::Not => {
                            if src_type == ScalarType::Float {
                                // Truthiness: compare float to 0.0
                                let zero = block.append_operation(arith::constant(
                                    context,
                                    FloatAttribute::new(context, f64_type, 0.0).into(),
                                    location,
                                ));
                                let zero_val: Value = zero.result(0).unwrap().into();
                                let cmp = block.append_operation(arith::cmpf(
                                    context,
                                    CmpfPredicate::Oeq,
                                    sv,
                                    zero_val,
                                    location,
                                ));
                                let i1_val: Value = cmp.result(0).unwrap().into();
                                let ext = block
                                    .append_operation(arith::extui(i1_val, i64_type, location));
                                (ext.result(0).unwrap().into(), ScalarType::Int)
                            } else {
                                let zero = block.append_operation(arith::constant(
                                    context,
                                    IntegerAttribute::new(i64_type, 0).into(),
                                    location,
                                ));
                                let zero_val: Value = zero.result(0).unwrap().into();
                                let cmp = block.append_operation(arith::cmpi(
                                    context,
                                    CmpiPredicate::Eq,
                                    sv,
                                    zero_val,
                                    location,
                                ));
                                let i1_val: Value = cmp.result(0).unwrap().into();
                                let ext = block
                                    .append_operation(arith::extui(i1_val, i64_type, location));
                                (ext.result(0).unwrap().into(), ScalarType::Int)
                            }
                        }
                        UnaryOp::BitNot => {
                            if src_type == ScalarType::Float {
                                return Err(
                                    "bitwise not on float operand not supported".to_string()
                                );
                            }
                            let neg1 = block.append_operation(arith::constant(
                                context,
                                IntegerAttribute::new(i64_type, -1).into(),
                                location,
                            ));
                            let neg1_val: Value = neg1.result(0).unwrap().into();
                            let xor = block.append_operation(arith::xori(sv, neg1_val, location));
                            (xor.result(0).unwrap().into(), ScalarType::Int)
                        }
                    };
                    regs.insert(dst.0, result);
                    types.insert(dst.0, result_type);
                }
                LirInstr::StoreLocal { slot, src } => {
                    let val = *regs
                        .get(&src.0)
                        .ok_or_else(|| format!("undefined reg r{} in StoreLocal", src.0))?;
                    let src_type = types.get(&src.0).copied().unwrap_or(ScalarType::Int);
                    // Memref slots are always i64; bitcast f64 → i64 for storage
                    let store_val = if src_type == ScalarType::Float {
                        let bc = block.append_operation(arith::bitcast(val, i64_type, location));
                        bc.result(0).unwrap().into()
                    } else {
                        val
                    };
                    let slot_ptr = *local_slots
                        .get(&(*slot as u32))
                        .ok_or_else(|| format!("unallocated local slot {}", slot))?;
                    block.append_operation(memref::store(store_val, slot_ptr, &[], location));
                    slot_types.insert(*slot as u32, src_type);
                }
                LirInstr::LoadLocal { dst, slot } => {
                    let slot_ptr = *local_slots
                        .get(&(*slot as u32))
                        .ok_or_else(|| format!("unallocated local slot {}", slot))?;
                    let load_op = block.append_operation(memref::load(slot_ptr, &[], location));
                    let loaded: Value = load_op.result(0).unwrap().into();
                    let slot_ty = slot_types
                        .get(&(*slot as u32))
                        .copied()
                        .unwrap_or(ScalarType::Int);
                    // Memref slots are i64; bitcast i64 → f64 if slot holds a float
                    let result = if slot_ty == ScalarType::Float {
                        let bc = block.append_operation(arith::bitcast(loaded, f64_type, location));
                        bc.result(0).unwrap().into()
                    } else {
                        loaded
                    };
                    regs.insert(dst.0, result);
                    types.insert(dst.0, slot_ty);
                }
                LirInstr::Convert { dst, op, src } => {
                    let sv = *regs
                        .get(&src.0)
                        .ok_or_else(|| format!("undefined reg r{}", src.0))?;
                    let src_type = types.get(&src.0).copied().unwrap_or(ScalarType::Int);
                    let (result, result_type) = match op {
                        ConvOp::IntToFloat => {
                            if src_type == ScalarType::Float {
                                (sv, ScalarType::Float)
                            } else {
                                let conv =
                                    block.append_operation(arith::sitofp(sv, f64_type, location));
                                (conv.result(0).unwrap().into(), ScalarType::Float)
                            }
                        }
                        ConvOp::FloatToInt => {
                            if src_type == ScalarType::Int {
                                (sv, ScalarType::Int)
                            } else {
                                let conv =
                                    block.append_operation(arith::fptosi(sv, i64_type, location));
                                (conv.result(0).unwrap().into(), ScalarType::Int)
                            }
                        }
                    };
                    regs.insert(dst.0, result);
                    types.insert(dst.0, result_type);
                }
                _ => return Err(format!("unsupported instruction: {:?}", si.instr)),
            }
        }

        // Terminator
        match &lir_block.terminator.terminator {
            Terminator::Return(reg) => {
                let val = *regs
                    .get(&reg.0)
                    .ok_or_else(|| format!("undefined reg r{} in return", reg.0))?;
                let ret_type = types.get(&reg.0).copied().unwrap_or(ScalarType::Int);
                // Function returns i64; bitcast f64 → i64 for float returns.
                // Bool is already i64 0/1 — no bitcast needed.
                let return_val = if ret_type.is_float() {
                    let bc = block.append_operation(arith::bitcast(val, i64_type, location));
                    bc.result(0).unwrap().into()
                } else {
                    val
                };
                // Consistency check: Float vs non-Float is a real conflict.
                // Bool vs Int are both i64, so no conflict.
                if let Some(prev) = return_type {
                    if prev.is_float() != ret_type.is_float() {
                        return Err("inconsistent return types across blocks".to_string());
                    }
                }
                // Prefer Bool if any return is Bool (for correct reboxing).
                return_type = Some(match (return_type, ret_type) {
                    (Some(ScalarType::Bool), _) | (_, ScalarType::Bool) => ScalarType::Bool,
                    _ => ret_type,
                });
                block.append_operation(func::r#return(&[return_val], location));
            }
            Terminator::Jump(label) => {
                let target_idx = label_to_idx
                    .get(&label.0)
                    .ok_or_else(|| format!("unknown label {}", label.0))?;
                block.append_operation(cf::br(&blocks[*target_idx], &[], location));
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let cond_i64 = *regs
                    .get(&cond.0)
                    .ok_or_else(|| format!("undefined reg r{} in branch", cond.0))?;
                // Compare to zero for truthiness (0=false, nonzero=true).
                // trunci would take the LSB, giving wrong results for even
                // nonzero values (e.g. 2 → false).
                let zero = block.append_operation(arith::constant(
                    context,
                    IntegerAttribute::new(i64_type, 0).into(),
                    location,
                ));
                let zero_val: Value = zero.result(0).unwrap().into();
                let cmp = block.append_operation(arith::cmpi(
                    context,
                    CmpiPredicate::Ne,
                    cond_i64,
                    zero_val,
                    location,
                ));
                let cond_val: Value = cmp.result(0).unwrap().into();
                let then_idx = *label_to_idx
                    .get(&then_label.0)
                    .ok_or_else(|| format!("unknown then label {}", then_label.0))?;
                let else_idx = *label_to_idx
                    .get(&else_label.0)
                    .ok_or_else(|| format!("unknown else label {}", else_label.0))?;
                block.append_operation(cf::cond_br(
                    context,
                    cond_val,
                    &blocks[then_idx],
                    &blocks[else_idx],
                    &[],
                    &[],
                    location,
                ));
            }
            _ => {
                return Err(format!(
                    "unsupported terminator: {:?}",
                    lir_block.terminator.terminator
                ))
            }
        }
    }

    for block in blocks {
        region.append_block(block);
    }

    let func_op = func::func(
        context,
        StringAttribute::new(context, func_name),
        TypeAttribute::new(func_type.into()),
        region,
        &[(
            melior::ir::Identifier::new(context, "llvm.emit_c_interface"),
            melior::ir::attribute::Attribute::unit(context),
        )],
        location,
    );
    module.body().append_operation(func_op);

    if !module.as_operation().verify() {
        return Err("MLIR verification failed".to_string());
    }

    Ok((module, return_type.unwrap_or(ScalarType::Int)))
}

/// Lower a GPU-eligible LirFunction to MLIR text (for debugging/testing).
pub fn lower_to_mlir(lir: &LirFunction) -> Result<String, String> {
    let context = create_context();
    let (module, _) = lower_to_module(&context, lir, 0, 0, 0)?;
    Ok(module.as_operation().to_string())
}
