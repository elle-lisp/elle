// audited: 2026-10-21
//! Lambda body compilation: state save and restore, environment layout, and the body frozen as a function of its own.
//!
//! src/lir/lower/AGENTS.md
//! docs/impl/lir.md

use crate::hir::Lambda;
use crate::lir::lower::*;

impl<'a> Lowerer<'a> {
    /// Lower a lambda body to a function of its own, `closure_id` in the
    /// unit's closure list, and answer it frozen.
    ///
    /// The builder sets the enclosing function aside, its open block included,
    /// and resumes it when this one finishes.
    pub(super) fn lower_lambda_body(
        &mut self,
        closure_id: ClosureId,
        lambda: &Lambda,
        rest_list_layout: crate::value::RestListLayout,
    ) -> Result<LirOwned, String> {
        let arity = lambda.arity();

        self.b().begin_function(arity);
        self.open_block(Label(0));
        // Taken, not read: only the lambda the binder named gets it, never a
        // nested anonymous one.
        let name = self.pending_lambda_name.take();
        let head = self.head();
        head.name = name;
        head.closure_id = Some(closure_id);
        head.num_captures = lambda.captures.len() as u16;
        head.doc = lambda.doc.clone();
        head.origin = lambda.origin;
        head.vararg_kind = lambda.vararg_kind.clone();
        head.rest_list_layout = rest_list_layout;
        head.num_params = lambda.params.len();
        // The body closed for a graft rides to the code payload as bytes, which
        // a later compile decodes once (docs/impl/graft.md).
        head.fragment = match &lambda.fragment {
            Some(fragment) => fragment.encode(),
            None => Vec::new(),
        };

        // Save state
        let saved_reg = self.next_reg;
        let saved_label = self.next_label;
        let saved_bindings = std::mem::take(&mut self.binding_to_slot);
        let saved_in_lambda = self.in_lambda;
        let saved_num_captures = self.num_captures;
        let saved_num_local_params = self.num_local_params;
        let saved_upvalue_bindings = std::mem::take(&mut self.upvalue_bindings);
        // Slot address spaces are per-function, so the compiled-cell set is too:
        // a binding of the enclosing body must not decide how a same-named slot
        // in this one is read.
        let saved_compiled_cell_bindings = std::mem::take(&mut self.compiled_cell_bindings);
        let saved_discard_slot = self.discard_slot;
        let saved_region_to_table = std::mem::take(&mut self.region_to_table);
        // `region_to_slot` lets `emit_decrefs_for` find the slot that holds a
        // region's value. Slot indices are per-function, so the map starts empty
        // here and the parent's map comes back on exit. A map left in place would
        // pair an outer call's region with a slot index of this function.
        let saved_region_to_slot = std::mem::take(&mut self.region_to_slot);
        // A tail-exit relocation point names an index into ONE block's
        // instruction list of ONE function. A fresh body starts at `Label(0)` and
        // block 0, as the enclosing one did. So put away the enclosing points and
        // the arm collection a branch mid-lowering is filling; a colliding label
        // or index would match them otherwise.
        let saved_tail_exit_hoist = std::mem::take(&mut self.tail_exit_hoist);
        let saved_arm_exit_hoists = std::mem::take(&mut self.arm_exit_hoists);
        // Reassigned-local slots are this function's local index space (per-
        // function, like `region_to_slot`), so reset for the new body.
        let saved_reassigned_local_slots = std::mem::take(&mut self.reassigned_local_slots);
        // `lower_define` and `lower_letrec` set the function context before they
        // lower a lambda initializer. Take it here; the body gets it back below.
        let saved_function_binding = self.current_function_binding.take();
        let saved_function_params = self.current_function_params.take();
        // The self-recursive binding of THIS lambda body: the binding this lambda
        // captures as `CaptureKind::Recursive` (a same-binding self-edge, classified
        // in src/hir/analyze/scopes.rs). A reference to it inside the body — in value OR
        // call position — resolves to the executing closure via `LoadSelf` / a
        // self-call (`lower_var`), never a cell load. Read directly from the classified
        // fact, independent of whether the binding also keeps a cell for a sibling
        // (`needs_capture()`): the self-edge is resolved by the executing closure in
        // every case. Saved/restored so a nested lambda restores the enclosing value.
        let saved_self_binding = self.current_self_binding.take();
        let this_self_binding: Option<Binding> = lambda.captures.iter().find_map(|cap| {
            if let crate::hir::CaptureKind::Recursive { binding } = cap.kind {
                Some(binding)
            } else {
                None
            }
        });

        // num_locals starts at 0; stack-held params and let-bound vars
        // will increment it as they're allocated.
        // Cell-held params go into the env (not counted in num_locals for stack frame).
        self.next_reg = 0;
        self.next_label = 1;
        self.discard_slot = None;
        self.in_lambda = true;
        self.num_captures = lambda.captures.len() as u16;
        self.num_local_params = 0;

        // In a closure, the environment is laid out as:
        // [captured_vars..., parameters..., locally_defined_cells...]
        // So:
        // - Captured variables are at indices [0, num_captures)
        // - Parameters are at indices [num_captures, num_captures + num_params)

        // Bind captured variables to upvalue indices
        for (i, cap) in lambda.captures.iter().enumerate() {
            self.binding_to_slot.insert(cap.binding, i as u16);
            self.upvalue_bindings.insert(cap.binding);
        }

        // Build capture_params_mask and bind parameters.
        // A cell-held param stays in the env as an upvalue (LoadCapture/StoreCapture).
        // Any other param is a local (LoadLocal/StoreLocal), copied from the env at entry.
        let mut capture_params_mask: u64 = 0;
        for (i, param) in lambda.params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();

            if needs_capture {
                if i < 64 {
                    capture_params_mask |= 1 << i;
                }
                // Cell-held param: lives in the env as an upvalue
                let upvalue_idx = self.num_captures + i as u16;
                self.binding_to_slot.insert(*param, upvalue_idx);
                self.upvalue_bindings.insert(*param);
            } else {
                // Stack-held param: allocate a local slot.
                // We'll copy from env into this local at function entry.
                let slot = self.fresh_local();
                self.num_local_params += 1;
                self.binding_to_slot.insert(*param, slot);
                // NOT added to upvalue_bindings → uses LoadLocal/StoreLocal
            }
        }
        self.head().capture_params_mask = capture_params_mask;

        // Copy stack-held params from the env into their local slots.
        // The VM/host populates the env as [captures..., params...],
        // so param i sits at env index (num_captures + i).
        for (i, param) in lambda.params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();
            if !needs_capture {
                let env_idx = self.num_captures + i as u16;
                let slot = *self.binding_to_slot.get(param).unwrap();
                let tmp = self.fresh_reg();
                self.emit(InstrRef::LoadCaptureRaw {
                    dst: tmp,
                    index: env_idx,
                });
                self.emit(InstrRef::StoreLocal { slot, src: tmp });
            }
        }

        let num_local_params = self.num_local_params as usize;
        self.head().num_local_params = num_local_params;

        // Each param is an OWNED binding: the analysis gave it a placeholder
        // region in `call_result_regions` (the Lambda arm of
        // src/hir/region/infer/walk.rs). Record `region_to_slot[param_r] = slot`
        // so `emit_decrefs_for` can
        // release it at the param's true last use. `binding_to_slot` already
        // holds the right slot for each kind:
        //   - non-captured param → a LOCAL slot; released `LoadLocal` +
        //     `DecrefValueRegion` (drops the arg's runtime region).
        //   - captured (cell-held) param → the UPVALUE/env index; its placeholder
        //     is in `cell_release_regions`, released `LoadCaptureRaw` +
        //     `DecrefCellRegion` (frees the env cell's own region).
        for param in lambda.params.iter() {
            let Some(&slot) = self.binding_to_slot.get(param) else {
                continue;
            };
            let space = self.value_slot_for(*param, slot);
            if let Some(regions) = self.region_info.binding_source_regions.get(param) {
                for &r in regions.clone().iter() {
                    if self.region_info.call_result_regions.contains(&r) {
                        self.region_to_slot.insert(r, space);
                    }
                }
            }
        }

        // Give the body back the function context taken above.
        self.current_function_binding = saved_function_binding;
        self.current_function_params = saved_function_params.clone();
        // This body's self-reference resolves to the executing closure (value
        // path). Set before lowering the body; restored below.
        self.current_self_binding = this_self_binding;

        // Emit signal bound checks for each bounded parameter
        for pb in &lambda.param_bounds {
            if let Some(&slot) = self.binding_to_slot.get(&pb.binding) {
                let src = self.fresh_reg();
                let is_upvalue = self.upvalue_bindings.contains(&pb.binding);
                if is_upvalue {
                    self.emit(InstrRef::LoadCapture {
                        dst: src,
                        index: slot,
                    });
                } else {
                    self.emit(InstrRef::LoadLocal { dst: src, slot });
                }
                self.emit(InstrRef::CheckSignalBound {
                    src,
                    allowed_bits: pb.signal.bits,
                });
            }
        }

        // Lower the body. Each `Return` the return-wrapping pass
        // (src/hir/return_incref.rs) placed mints the caller's reference, so
        // the body's own releases fire at their `decref_point`s as usual.
        let result_reg = self.lower_expr(&lambda.body)?;

        // Fallback release for UNUSED non-captured params. A used param's
        // placeholder region gets a `decref_point` (from its uses) and is
        // released by `emit_decrefs_for`; an unused param has no `region_data`
        // entry, so without this its moved-in arg would leak. No return mint
        // precedes this release: an unused param is never the return value.
        //
        // A param the body's tail call MOVES is released by the callee instead.
        // So the release goes through the relocation wrapper rather than straight
        // out. When the body ends in a frame-replacing tail call, this emission
        // point is dead, so the release is carried back ahead of the `TailCall`
        // for every param the call does not name. An unused param is by
        // definition not one of its arguments. The exemption is what keeps a
        // param used ONLY as such an argument out of the hoist
        // (docs/impl/region/mechanism.md).
        //
        // The nil stamp makes the release SELF-CANCELLING, so it can be
        // replicated into the arms of a branch the body ends in. Whichever copy a
        // path reaches first blanks the slot, and a later copy loads `nil` and
        // no-ops. Blanking is free here — the param is used nowhere, so nothing
        // reads the slot again.
        let unused_params: Vec<(u16, crate::hir::region::Region)> = lambda
            .params
            .iter()
            .filter(|p| !self.arena.get(**p).needs_capture())
            .filter_map(|p| {
                let slot = *self.binding_to_slot.get(p)?;
                let regions = self.region_info.binding_source_regions.get(p)?;
                let unreleased = regions.iter().copied().find(|r| {
                    self.region_info.call_result_regions.contains(r)
                        && !self.region_info.region_data.contains_key(r)
                })?;
                Some((slot, unreleased))
            })
            .collect();
        for (slot, region) in unused_params {
            self.with_tail_exit_hoist(region, |s| {
                let val_reg = s.fresh_reg();
                s.emit(InstrRef::LoadLocal { dst: val_reg, slot });
                s.emit(InstrRef::DecrefValueRegion { src: val_reg });
                if let Ok(nil_reg) = s.emit_const(ConstRef::Nil) {
                    s.emit(InstrRef::StoreLocal { slot, src: nil_reg });
                }
            });
        }

        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        let num_regs = self.next_reg;
        let head = self.head();
        head.entry = Label(0);
        head.num_regs = num_regs;
        head.signal = lambda.inferred_signals;

        self.current_function_binding = None;
        self.current_function_params = None;

        // Record this lambda's merged slots while `region_to_table` still holds
        // its slots (restored to the parent's below). Empty unless a builder-idiom
        // merge fired in this lambda — see `record_merged_slots`.
        self.record_merged_slots();

        let func = self.b().finish_function()?;

        // Restore state
        self.next_reg = saved_reg;
        self.next_label = saved_label;
        self.binding_to_slot = saved_bindings;
        self.in_lambda = saved_in_lambda;
        self.num_captures = saved_num_captures;
        self.num_local_params = saved_num_local_params;
        self.upvalue_bindings = saved_upvalue_bindings;
        self.compiled_cell_bindings = saved_compiled_cell_bindings;
        self.discard_slot = saved_discard_slot;
        self.region_to_table = saved_region_to_table;
        self.region_to_slot = saved_region_to_slot;
        self.reassigned_local_slots = saved_reassigned_local_slots;
        self.current_self_binding = saved_self_binding;
        self.tail_exit_hoist = saved_tail_exit_hoist;
        self.arm_exit_hoists = saved_arm_exit_hoists;

        Ok(func)
    }
}
