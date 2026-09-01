//! LIR to Cranelift IR translation
//!
//! This module contains `FunctionTranslator`, which translates individual
//! LIR instructions and terminators to Cranelift IR.
//!
//! ## Variable layout
//!
//! Each LIR register `r` maps to TWO Cranelift variables:
//!   - tag:     `Variable::from_u32(2 * r)`
//!   - payload: `Variable::from_u32(2 * r + 1)`
//!
//! Arg variables and local variables use the same doubling scheme starting
//! at their respective bases.
//!
//! This root holds the translator type and its register→variable mapping. The
//! per-concern lowering lives in submodules: `instr` (per-`LirInstr` match),
//! `terminator` (per-`Terminator` match + tail-call dispatch), `region`
//! (prologue region-map/ctx plumbing), and `spill` (the shared spill slot).

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I32, I64};
use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::hir::region::StaticRegion;
use crate::lir::{Label, LirInstr, Reg, Terminator};
use crate::value::repr::{TAG_FALSE, TAG_NIL, TAG_TRUE};
use crate::value::SymbolId;

use super::vtable::RuntimeHelpers;
use super::JitError;

mod instr;
mod region;
mod spill;
mod terminator;

/// Translator for a single function
pub(crate) struct FunctionTranslator<'a> {
    pub(crate) module: &'a mut JITModule,
    pub(crate) helpers: &'a RuntimeHelpers,
    pub(crate) lir: &'a crate::lir::LirFunction,
    pub(crate) env_ptr: Option<cranelift_codegen::ir::Value>,
    pub(crate) vm_ptr: Option<cranelift_codegen::ir::Value>,
    /// Address of this activation's `JitCtx` capability bundle, built in the
    /// prologue (a stack slot holding `vm_ptr`). Threaded to the intrinsic
    /// fast-path helpers so they resolve the VM from it (docs/impl/region/ctx.md).
    /// `None` until the prologue runs.
    pub(crate) jit_ctx_ptr: Option<cranelift_codegen::ir::Value>,
    /// (tag, payload) Cranelift values for the closure being executed
    /// (for self-tail-call detection)
    pub(crate) self_tag_payload:
        Option<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)>,
    /// Base index for arg variables (= num_regs), in LIR register space
    pub(crate) arg_var_base: u32,
    /// Base index for locally-defined variable Cranelift variables
    pub(crate) local_var_base: u32,
    /// Loop header block for self-tail-call jumps
    pub(crate) loop_header: Option<cranelift_codegen::ir::Block>,
    /// SCC peer functions
    pub(crate) scc_peers: HashMap<SymbolId, FuncId>,
    /// Map from register to the SymbolId it was loaded from.
    pub(crate) global_load_map: HashMap<Reg, SymbolId>,
    /// SymbolId of the function being compiled (for self-call detection)
    pub(crate) self_sym: Option<SymbolId>,
    /// Counter for yield point indices
    pub(crate) yield_point_index: u32,
    /// Counter for call site indices
    pub(crate) call_site_index: u32,
    /// Shared stack slot for spilling locals + operands at yield/call sites.
    pub(crate) shared_spill_slot: Option<cranelift_codegen::ir::StackSlot>,
    /// The abandoned-frame release tables, materialized once in the prologue and
    /// handed to `elle_jit_release_abandoned_frame` at every error exit: the
    /// value routes' local slots (`u16`), the slot routes' static region ids
    /// (`u32`), and the scratch the locals spill into. `None` on each where the
    /// function's matching table is empty (docs/impl/region/mechanism.md § "An
    /// abandoned frame runs the releases it still owes").
    pub(crate) abandoned_slots_table: Option<cranelift_codegen::ir::StackSlot>,
    pub(crate) abandoned_regions_table: Option<cranelift_codegen::ir::StackSlot>,
    pub(crate) abandoned_locals_spill: Option<cranelift_codegen::ir::StackSlot>,
    /// Nested-lambda template **blueprints** built during MakeClosure
    /// translation. The native code holds a raw pointer to each (like
    /// `templates` below), and `elle_jit_make_closure` materializes a FRESH
    /// region-allocated `HeapObject::ClosureTemplate` from it per execution —
    /// reclaimed by region RC, never pinned for the process lifetime.
    /// `Box` gives each a stable heap address independent of this Vec's growth.
    #[allow(clippy::vec_box)] // the Box stable-address is the point (see above)
    pub(crate) closure_protos: Vec<Box<crate::value::ClosureTemplate>>,
    /// Immutable heap-literal templates baked by `MaterializeConst` (a string, or
    /// a quoted compound structure). The native code holds a raw pointer to each
    /// `ConstTemplate`, so they must outlive the JIT code; ownership is
    /// transferred to `JitCode` (the code object) after compilation. `Box` gives
    /// each template a stable heap address independent of this Vec's growth.
    #[allow(clippy::vec_box)] // the Box stable-address is the point (see above)
    pub(crate) templates: Vec<Box<crate::value::ConstTemplate>>,
    /// Symbol name map for nested emitters (MakeClosure).
    /// Module's closure list for MakeClosure → ClosureId lookup.
    pub(crate) module_closures: Vec<crate::lir::LirFunction>,
    /// Whether this function's LIR carries an `AdoptIntoActivation` — computed
    /// once at construction so the `Return` path emits the owner-node release
    /// (`elle_jit_release_activation_owner_node`) only for a function that can
    /// have minted a node; the common path pays no extra helper call.
    pub(crate) uses_activation_owner_node: bool,
}

/// Load the `index`-th `Value` of a runtime-owned array of `Value`s — an
/// argument array, or a closure environment — as its (tag, payload) halves.
///
/// This is the JIT's only site for memory flags; see docs/impl/jit.md
/// § "Memory flags on emitted loads".
pub(crate) fn load_value_slot(
    builder: &mut FunctionBuilder,
    base: cranelift_codegen::ir::Value,
    index: u32,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    const STRIDE: i32 = std::mem::size_of::<crate::value::Value>() as i32;
    const TAG: i32 = std::mem::offset_of!(crate::value::Value, tag) as i32;
    const PAYLOAD: i32 = std::mem::offset_of!(crate::value::Value, payload) as i32;

    let flags = MemFlagsData::trusted();
    let slot = index as i32 * STRIDE;
    let tag = builder.ins().load(I64, flags, base, slot + TAG);
    let payload = builder.ins().load(I64, flags, base, slot + PAYLOAD);
    (tag, payload)
}

impl<'a> FunctionTranslator<'a> {
    pub(crate) fn new(
        module: &'a mut JITModule,
        helpers: &'a RuntimeHelpers,
        lir: &'a crate::lir::LirFunction,
    ) -> Self {
        let uses_activation_owner_node = lir.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|si| matches!(si.instr, LirInstr::AdoptIntoActivation { .. }))
        });
        FunctionTranslator {
            module,
            helpers,
            lir,
            env_ptr: None,
            vm_ptr: None,
            jit_ctx_ptr: None,
            self_tag_payload: None,
            arg_var_base: 0,
            local_var_base: 0,
            loop_header: None,
            scc_peers: HashMap::new(),
            global_load_map: HashMap::new(),
            self_sym: None,
            yield_point_index: 0,
            call_site_index: 0,
            shared_spill_slot: None,
            abandoned_slots_table: None,
            abandoned_regions_table: None,
            abandoned_locals_spill: None,
            closure_protos: Vec::new(),
            templates: Vec::new(),
            module_closures: Vec::new(),
            uses_activation_owner_node,
        }
    }

    /// Convert LIR register index to the Cranelift variable index for its tag.
    /// Variable layout: 2*r = tag, 2*r+1 = payload.
    #[inline]
    pub(crate) fn var_tag(&self, r: u32) -> u32 {
        2 * r
    }

    /// Convert LIR register index to the Cranelift variable index for its payload.
    #[inline]
    pub(crate) fn var_payload(&self, r: u32) -> u32 {
        2 * r + 1
    }
}
