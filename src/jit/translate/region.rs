//! Prologue plumbing: region-map push/pop, local-variable init, per-slot
//! alloc-region resolution, and the `JitCtx` capability-bundle accessor.
//!
//! These methods bracket every activation — the prologue push and the balanced
//! pop-before-return keep the activation's region-remap frame paired, and
//! `init_locally_defined_vars` mirrors the interpreter's captured-local cell
//! minting so the JIT and interpreter agree on which locals get their own
//! per-execution region.

use super::*;

/// Helper to create a Cranelift Variable from a slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

impl<'a> FunctionTranslator<'a> {
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

    /// Materialize this function's two abandoned-frame release tables into stack
    /// slots, and reserve the scratch the value route's locals spill into. Both
    /// tables are compile-time constants, so the prologue writes them once and
    /// every error exit passes the same addresses to the runtime walk
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A function that owes nothing at an error exit gets neither
    /// slot and emits no walk at all.
    pub(crate) fn emit_abandoned_tables(&mut self, builder: &mut FunctionBuilder) {
        // Read the function out of `self` first: the loops below borrow it while
        // the writes at the end need `&mut self`.
        let lir = self.lir;
        let slots = &lir.frame_release_slots;
        if !slots.is_empty() {
            let table = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (slots.len() * 2) as u32,
                1,
            ));
            for (i, slot) in slots.iter().enumerate() {
                let c = builder
                    .ins()
                    .iconst(cranelift_codegen::ir::types::I16, *slot as i64);
                builder.ins().stack_store(c, table, (i * 2) as i32);
            }
            self.abandoned_slots_table = Some(table);
            // The value route reads slot `s` out of `locals[s]`, so the scratch
            // is indexed by slot and covers all of them, not just the released
            // ones (`Value` is 16 bytes).
            self.abandoned_locals_spill = Some(builder.create_sized_stack_slot(
                cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    (lir.num_locals as u32) * 16,
                    0,
                ),
            ));
        }
        let regions = &lir.frame_release_regions;
        if !regions.is_empty() {
            let table = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (regions.len() * 4) as u32,
                2,
            ));
            for (i, region) in regions.iter().enumerate() {
                let c = builder.ins().iconst(I32, region.get() as i64);
                builder.ins().stack_store(c, table, (i * 4) as i32);
            }
            self.abandoned_regions_table = Some(table);
        }
    }

    /// Emit the abandoned-frame walk for an activation leaving by an **error**:
    /// spill the frame's locals in slot order and hand them, with the prologue's
    /// tables, to `elle_jit_release_abandoned_frame`. A no-op for a function
    /// whose tables are both empty.
    ///
    /// Runs BEFORE the region-map pop: the slot route's receipt is this
    /// activation's map, which the pop discards.
    fn emit_release_abandoned_frame(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        if self.abandoned_slots_table.is_none() && self.abandoned_regions_table.is_none() {
            return Ok(());
        }
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("release_abandoned_frame without vm pointer".to_string())
        })?;
        let null = builder.ins().iconst(I64, 0);

        let (slots_ptr, num_slots) = match self.abandoned_slots_table {
            Some(table) => (
                builder.ins().stack_addr(I64, table, 0),
                builder
                    .ins()
                    .iconst(I64, self.lir.frame_release_slots.len() as i64),
            ),
            None => (null, null),
        };
        let (locals_ptr, num_locals) = match self.abandoned_locals_spill {
            Some(spill) => {
                for i in 0..self.lir.num_locals as u32 {
                    let (tag, payload) = self.use_var_pair(builder, self.local_var_base + i);
                    builder.ins().stack_store(tag, spill, (i * 16) as i32);
                    builder
                        .ins()
                        .stack_store(payload, spill, (i * 16 + 8) as i32);
                }
                (
                    builder.ins().stack_addr(I64, spill, 0),
                    builder.ins().iconst(I64, self.lir.num_locals as i64),
                )
            }
            None => (null, null),
        };
        let (regions_ptr, num_regions) = match self.abandoned_regions_table {
            Some(table) => (
                builder.ins().stack_addr(I64, table, 0),
                builder
                    .ins()
                    .iconst(I64, self.lir.frame_release_regions.len() as i64),
            ),
            None => (null, null),
        };

        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.release_abandoned_frame, builder.func);
        builder.ins().call(
            func_ref,
            &[
                vm,
                slots_ptr,
                num_slots,
                regions_ptr,
                num_regions,
                locals_ptr,
                num_locals,
            ],
        );
        Ok(())
    }

    /// Leave a compiled activation by an **error**: run the releases it still
    /// owed, then pop its region-remap frame and return. Every error exit goes
    /// through here — one that returns directly leaks whatever the frame's
    /// release tables name.
    pub(crate) fn emit_abandoned_error_return(
        &mut self,
        builder: &mut FunctionBuilder,
        rt: cranelift_codegen::ir::Value,
        rp: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        self.emit_release_abandoned_frame(builder)?;
        self.emit_pop_then_return(builder, rt, rp)
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

    /// Emit `elle_jit_resolve_alloc_region(vm, slot)` — resolve this allocation's
    /// per-slot physical region through this activation's region map and return its
    /// raw id (I32), to be passed directly to the alloc helper as its `region`
    /// argument.
    // `pub(super)` (was private in the translate root): sibling `instr`
    // submodules call this; a private item on a sibling module is not visible
    // to them, so widen to the minimal `translate`-scoped visibility.
    pub(super) fn emit_resolve_alloc_region(
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
}
