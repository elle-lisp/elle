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

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I32, I64};
use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::hir::region::StaticRegion;
use crate::lir::{Label, LirInstr, Reg, Terminator};
use crate::value::repr::{TAG_FALSE, TAG_NIL, TAG_TRUE};
use crate::value::SymbolId;

use super::vtable::RuntimeHelpers;
use super::JitError;

/// Helper to create a Cranelift Variable from a slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

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
    pub(crate) symbol_names: HashMap<u32, String>,
    /// Module's closure list for MakeClosure → ClosureId lookup.
    pub(crate) module_closures: Vec<crate::lir::LirFunction>,
    /// Whether this function's LIR carries an `AdoptIntoActivation` — computed
    /// once at construction so the `Return` path emits the owner-node release
    /// (`elle_jit_release_activation_owner_node`) only for a function that can
    /// have minted a node; the common path pays no extra helper call.
    pub(crate) uses_activation_owner_node: bool,
}

mod instr;

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
            closure_protos: Vec::new(),
            templates: Vec::new(),
            symbol_names: HashMap::new(),
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

    /// Initialize locally-defined variables.
    pub(crate) fn init_locally_defined_vars(
        &mut self,
        builder: &mut FunctionBuilder,
        num_locally_defined: u32,
    ) -> Result<(), JitError> {
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let zero = builder.ins().iconst(I64, 0);
        let capture_locals_mask = &self.lir.capture_locals_mask;

        // The first num_local_params slots are non-LBox param copies
        // (initialized at function entry). capture_locals_mask indexes from
        // the first let-bound local (after param copies).
        let nlp = self.lir.num_local_params as u32;

        for i in 0..num_locally_defined {
            let base = self.local_var_base + i;
            let mask_bit = i.saturating_sub(nlp);
            // Precise at any index: only genuinely captured locals get a cell.
            // No `mask_bit >= 64` fallback (which celled — and leaked — every
            // uncaptured high local; the JIT prologue mirrors the interpreter).
            let needs_capture = i >= nlp && capture_locals_mask.is_set(mask_bit as usize);
            if needs_capture {
                // Prologue env cell → its OWN fresh per-execution region
                // (`make_capture_owned`), mirroring the interpreter's
                // captured-local path in `populate_env` (`env_value_region`).
                // NOT `make_capture`, which would not give the cell its own
                // per-execution region.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("make_capture_owned without vm pointer".to_string())
                })?;
                let (cell_tag, cell_payload) = self.call_helper_value_vm(
                    builder,
                    self.helpers.make_capture_owned,
                    nil_tag,
                    zero,
                    vm,
                )?;
                builder.def_var(var(self.var_tag(base)), cell_tag);
                builder.def_var(var(self.var_payload(base)), cell_payload);
            } else {
                builder.def_var(var(self.var_tag(base)), nil_tag);
                builder.def_var(var(self.var_payload(base)), zero);
            }
        }

        Ok(())
    }

    /// Emit a call to `elle_jit_push_region_map(vm)` — push this activation's
    /// fresh region-remap frame. Emitted once in the function prologue.
    pub(crate) fn emit_push_region_map(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("push_region_map without vm pointer".to_string())
        })?;
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.push_region_map, builder.func);
        builder.ins().call(func_ref, &[vm]);
        Ok(())
    }

    /// Emit a call to `elle_jit_pop_region_map(vm)` — pop this activation's
    /// region-remap frame. Emitted before every `return`.
    fn emit_pop_region_map(&mut self, builder: &mut FunctionBuilder) -> Result<(), JitError> {
        let vm = self
            .vm_ptr
            .ok_or_else(|| JitError::InvalidLir("pop_region_map without vm pointer".to_string()))?;
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.pop_region_map, builder.func);
        builder.ins().call(func_ref, &[vm]);
        Ok(())
    }

    /// Pop the region-remap frame, then emit the function `return`. Every exit
    /// path goes through here so the prologue push is always balanced. The pop
    /// is correct on the yield/Emit side-exit too: the suspend helper has
    /// already cloned the map into the resume frame before this runs.
    pub(crate) fn emit_pop_then_return(
        &mut self,
        builder: &mut FunctionBuilder,
        rt: cranelift_codegen::ir::Value,
        rp: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        self.emit_pop_region_map(builder)?;
        builder.ins().return_(&[rt, rp]);
        Ok(())
    }

    /// Branch on a generic (helper-dispatched) tail call's runtime result so
    /// the JIT mirrors the interpreter's `tail_call_inner` (src/vm/call.rs):
    ///
    /// - `TAIL_CALL_SENTINEL` (callee was a closure → the trampoline runs it),
    ///   `YIELD_SENTINEL` (a yielding native side-exited), or a pending error:
    ///   pop the region map and return the value, exactly as before. The
    ///   post-`TailCall` owned-arg releases do NOT run — for a closure that is
    ///   the ownership MOVE (the owned-param callee releases the moved args),
    ///   and on error/yield the frame unwinds/suspends.
    /// - any other value: the callee was a NATIVE (or a parameter/collection)
    ///   that completed normally. Bind `dst` and fall through so the caller
    ///   keeps translating the post-`TailCall` block — the compiler's own
    ///   per-arg `DecrefValueRegion`/`DecrefRegion`s that release each moved
    ///   native arg. This is the Inc4 native-tail trick the interpreter
    ///   performs by NOT replacing the frame for a normally-completing native;
    ///   without it the moved arg leaks (region-native-tail-move.lisp;
    ///   docs/impl/region/rules.md Rule 8).
    ///
    /// On return the builder is positioned on the continue (fall-through)
    /// block, with `dst` defined.
    fn emit_tail_call_result_branch(
        &mut self,
        builder: &mut FunctionBuilder,
        dst: Reg,
        rt: cranelift_codegen::ir::Value,
        rp: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        use crate::jit::value::{TAIL_CALL_SENTINEL_JV, YIELD_SENTINEL_JV};

        // result == TAIL_CALL_SENTINEL || result == YIELD_SENTINEL? The sentinel
        // tags are 0xDEAD_…_DEAD — unrepresentable as a real Value tag (> the
        // max tag), so the tag alone identifies a sentinel (the payload mirrors
        // the tag, no need to compare it).
        let tail_tag = builder.ins().iconst(I64, TAIL_CALL_SENTINEL_JV.tag as i64);
        let yield_tag = builder.ins().iconst(I64, YIELD_SENTINEL_JV.tag as i64);
        let is_tail = builder.ins().icmp(IntCC::Equal, rt, tail_tag);
        let is_yield = builder.ins().icmp(IntCC::Equal, rt, yield_tag);
        let is_sentinel = builder.ins().bor(is_tail, is_yield);

        let return_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_sentinel, return_block, &[], cont_block, &[]);

        // Closure-trampoline / yield side-exit: return the value unchanged.
        builder.switch_to_block(return_block);
        builder.seal_block(return_block);
        self.emit_pop_then_return(builder, rt, rp)?;

        // Native (or param/collection) completed normally: bind dst, propagate
        // any pending error, then fall through into the post-`TailCall` block.
        // `emit_exception_check_after_call` returns nil if the native set an
        // error signal and leaves the builder on its own continue block.
        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);
        self.def_var_pair(builder, dst.0, rt, rp);
        self.emit_exception_check_after_call(builder)?;
        Ok(())
    }

    /// Emit `elle_jit_resolve_alloc_region(vm, slot)` — resolve this allocation's
    /// per-slot physical region through this activation's region map and return its
    /// raw id (I32), to be passed directly to the alloc helper as its `region`
    /// argument.
    fn emit_resolve_alloc_region(
        &mut self,
        builder: &mut FunctionBuilder,
        slot: StaticRegion,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("resolve_alloc_region without vm pointer".to_string())
        })?;
        let slot_const = builder.ins().iconst(I32, slot.get() as i64);
        // A slot a builder-idiom merge collapsed allocations onto (recorded in this
        // function's `merged_slots`) routes to the mint-or-reuse helper, so the
        // child mints and the parent reuses one physical region — the JIT mirror of
        // the interpreter's `runtime_region_for_alloc_slot_maybe_merged`. The
        // membership is decided here, at compile time, so the hot path carries no
        // set lookup (docs/impl/region/merging.md § Merging).
        let helper = if self.lir.merged_slots.contains(&slot) {
            self.helpers.resolve_alloc_region_merged
        } else {
            self.helpers.resolve_alloc_region
        };
        let func_ref = self.module.declare_func_in_func(helper, builder.func);
        let call = builder.ins().call(func_ref, &[vm, slot_const]);
        Ok(builder.inst_results(call)[0])
    }

    /// The address of this activation's `JitCtx` capability bundle (built in the
    /// prologue), to thread as the trailing argument of an intrinsic fast-path
    /// helper so it resolves the VM from the bundle.
    pub(crate) fn jit_ctx(&self) -> Result<cranelift_codegen::ir::Value, JitError> {
        self.jit_ctx_ptr
            .ok_or_else(|| JitError::InvalidLir("intrinsic without jit_ctx pointer".to_string()))
    }

    /// Translate a terminator
    pub(crate) fn translate_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match term {
            Terminator::Return(reg) => {
                let (tag, payload) = self.use_var_pair(builder, reg.0);
                // Free this activation's owner node at normal completion — the
                // JIT twin of the interpreter trampoline's clean-break release
                // (docs/impl/region/owner.md § "Owner nodes"). Emitted before
                // the region-map pop, mirroring the interpreter's ordering, and
                // only for a function whose LIR can mint a node.
                if self.uses_activation_owner_node {
                    let vm = self.vm_ptr.ok_or_else(|| {
                        JitError::InvalidLir("owner-node release without vm pointer".to_string())
                    })?;
                    let func_ref = self.module.declare_func_in_func(
                        self.helpers.release_activation_owner_node,
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[vm]);
                }
                self.emit_pop_then_return(builder, tag, payload)?;
            }

            Terminator::Jump(label) => {
                let target = block_map.get(label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown jump target: {:?}", label))
                })?;
                builder.ins().jump(*target, &[]);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let (cond_tag, _) = self.use_var_pair(builder, cond.0);
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Truthiness: tag != TAG_NIL (2) AND tag != TAG_FALSE (4)
                // Equivalently: is_truthy if tag != NIL and tag != FALSE.
                // Simple check: tag == TAG_FALSE || tag == TAG_NIL → falsy
                let tag_nil = builder.ins().iconst(I64, TAG_NIL as i64);
                let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
                let is_nil = builder.ins().icmp(IntCC::Equal, cond_tag, tag_nil);
                let is_false = builder.ins().icmp(IntCC::Equal, cond_tag, tag_false);
                let is_falsy = builder.ins().bor(is_nil, is_false);
                // brif on is_falsy goes to else, otherwise then
                builder
                    .ins()
                    .brif(is_falsy, *else_block, &[], *then_block, &[]);
            }

            Terminator::Emit {
                signal,
                value,
                resume_label: _,
            } => {
                let (yt, yp) = self.use_var_pair(builder, value.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Emit without vm pointer".to_string()))?;
                let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
                    JitError::InvalidLir("Emit without self_tag_payload".to_string())
                })?;

                let yield_index = self.yield_point_index;
                self.yield_point_index += 1;

                let stack_regs = self
                    .lir
                    .yield_points
                    .get(yield_index as usize)
                    .map(|yp| yp.stack_regs.as_slice())
                    .unwrap_or(&[]);

                let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;
                let yield_idx_val = builder.ins().iconst(I64, yield_index as i64);

                let sig_val = builder.ins().iconst(I64, signal.raw() as i64);

                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.jit_yield, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[
                        yt,
                        yp,
                        spilled_ptr,
                        yield_idx_val,
                        vm,
                        self_tag,
                        self_payload,
                        sig_val,
                    ],
                );
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                // Pop AFTER elle_jit_yield: the suspend helper has already cloned
                // the live activation map into the resume frame.
                self.emit_pop_then_return(builder, rt, rp)?;
            }

            Terminator::Unreachable => {
                // User trap code 1 — `unwrap_user(0)` panics (Cranelift user
                // trap codes are `NonZeroU8`). Reachable now that a generic
                // tail call can fall through to its block's terminator instead
                // of self-terminating (the native-tail continue path); a
                // genuinely-unreachable block must still compile to a valid
                // trap.
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            }
        }
        Ok(())
    }

    /// Allocate the shared spill slot sized to the maximum spill requirement.
    pub(crate) fn allocate_shared_spill_slot(&mut self, builder: &mut FunctionBuilder) {
        let num_locals = self.lir.num_locals as usize;

        let max_yield_operands = self
            .lir
            .yield_points
            .iter()
            .map(|yp| yp.stack_regs.len())
            .max()
            .unwrap_or(0);
        let max_call_operands = self
            .lir
            .call_sites
            .iter()
            .map(|cs| cs.stack_regs.len())
            .max()
            .unwrap_or(0);
        let max_operands = std::cmp::max(max_yield_operands, max_call_operands);
        // Spill saves: arity params (arg_vars) + num_locals locals (local_var_base)
        // + operand stack entries.
        let arity = self.lir.num_params;
        let max_total = arity + num_locals + max_operands;

        if max_total > 0 {
            // Each Value is 16 bytes
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (max_total * 16) as u32,
                0,
            ));
            self.shared_spill_slot = Some(slot);
        }
    }

    /// Spill local variables and operand stack registers to the shared stack slot.
    ///
    /// Returns a Cranelift value pointing to the spilled buffer (*const Value),
    /// or a null pointer constant if there's nothing to spill.
    pub(crate) fn spill_locals_and_operands(
        &mut self,
        builder: &mut FunctionBuilder,
        stack_regs: &[Reg],
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let arity = self.lir.num_params as u16;
        let num_locals = self.lir.num_locals;
        // Spill params (from arg vars) + all local vars (from local_var_base).
        // num_locals includes non-LBox param copies + let-bound locals.
        let num_locally_defined = num_locals;
        let total = arity as usize + num_locally_defined as usize + stack_regs.len();

        if total == 0 {
            return Ok(builder.ins().iconst(I64, 0)); // null pointer
        }

        let slot = self
            .shared_spill_slot
            .expect("JIT bug: spill_locals_and_operands called but no shared spill slot allocated");

        let mut slot_idx: i32 = 0;

        // 1. Spill parameters (from arg variables)
        for i in 0..arity as u32 {
            let base = self.arg_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        // 2. Spill locally-defined variables
        for i in 0..num_locally_defined as u32 {
            let base = self.local_var_base + i;
            let (tag, payload) = self.use_var_pair(builder, base);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        // 3. Spill operand stack registers
        for reg in stack_regs {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            let tag_offset = slot_idx * 16;
            let payload_offset = slot_idx * 16 + 8;
            builder.ins().stack_store(tag, slot, tag_offset);
            builder.ins().stack_store(payload, slot, payload_offset);
            slot_idx += 1;
        }

        Ok(builder.ins().stack_addr(I64, slot, 0))
    }

    /// Helper: emit a tail call with args spilled to stack.
    fn emit_tail_call_with_args(
        &mut self,
        builder: &mut FunctionBuilder,
        ft: cranelift_codegen::ir::Value,
        fp: cranelift_codegen::ir::Value,
        args: &[Reg],
        vm: cranelift_codegen::ir::Value,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        if args.is_empty() {
            let null_ptr = builder.ins().iconst(I64, 0);
            let nargs = builder.ins().iconst(I64, 0);
            self.call_helper_tail_call(builder, ft, fp, null_ptr, nargs, vm, region_id_const)
        } else {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (args.len() * 16) as u32,
                0,
            ));
            for (i, arg_reg) in args.iter().enumerate() {
                let (at, ap) = self.use_var_pair(builder, arg_reg.0);
                let tag_offset = (i * 16) as i32;
                let payload_offset = (i * 16 + 8) as i32;
                builder.ins().stack_store(at, slot, tag_offset);
                builder.ins().stack_store(ap, slot, payload_offset);
            }
            let args_addr = builder.ins().stack_addr(I64, slot, 0);
            let nargs = builder.ins().iconst(I64, args.len() as i64);
            self.call_helper_tail_call(builder, ft, fp, args_addr, nargs, vm, region_id_const)
        }
    }
}
