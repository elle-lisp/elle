//! Lambda body compilation: saves/restores the lowerer's per-function state,
//! lays out the closure environment (captures, params, locals), and lowers the
//! body into a self-contained `LirFunction`.

use crate::hir::{CaptureInfo, ParamBound};
use crate::lir::lower::*;
use crate::value::Arity;

impl<'a> Lowerer<'a> {
    /// Lower a lambda body to a separate LirFunction.
    ///
    /// `pub(super)` so the sibling `expr` submodule (`lower_lambda_expr`) can
    /// reach it; it was a module-private `fn` when both lived in one file.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_lambda_body(
        &mut self,
        params: &[Binding],
        num_required: usize,
        rest_param: Option<&Binding>,
        vararg_kind: &crate::hir::VarargKind,
        captures: &[CaptureInfo],
        body: &Hir,
        _num_locals: u16,
        inferred_signal: crate::signals::Signal,
        param_bounds: &[ParamBound],
        doc: Option<std::rc::Rc<str>>,
        origin: Option<crate::syntax::Span>,
    ) -> Result<LirFunction, String> {
        // Compute arity
        let arity = Arity::for_lambda(rest_param.is_some(), num_required, params.len());

        // Save state
        let saved_func = std::mem::replace(&mut self.current_func, LirFunction::new(arity));
        let saved_block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        let saved_reg = self.next_reg;
        let saved_label = self.next_label;
        let saved_bindings = std::mem::take(&mut self.binding_to_slot);
        let saved_in_lambda = self.in_lambda;
        let saved_num_captures = self.num_captures;
        let saved_num_local_params = self.num_local_params;
        let saved_upvalue_bindings = std::mem::take(&mut self.upvalue_bindings);
        let saved_discard_slot = self.discard_slot;
        let saved_region_to_table = std::mem::take(&mut self.region_to_table);
        // `region_to_slot` is the post-ANF replacement for the
        // retired `call_region_slot` shadow map: it lets
        // `emit_decrefs_for` find the slot owning a
        // call_result_region. Slots are per-function (LIR's local
        // slot index space is per-function), so the map must be
        // empty inside the new lambda body and the parent's map
        // must be restored on exit. Without this, an outer Call's
        // region could be associated with a stale slot index from
        // the inner function.
        let saved_region_to_slot = std::mem::take(&mut self.region_to_slot);
        // A tail-exit relocation point names an index into ONE block's
        // instruction list of ONE function, and a fresh body starts at `Label(0)`
        // and block 0 exactly as the enclosing one did — so the enclosing points,
        // and the arm collection a branch mid-lowering is filling, must be put
        // away rather than left to be matched by a colliding label or index.
        let saved_tail_exit_hoist = std::mem::take(&mut self.tail_exit_hoist);
        let saved_arm_exit_hoists = std::mem::take(&mut self.arm_exit_hoists);
        // Reassigned-local slots are this function's local index space (per-
        // function, like `region_to_slot`), so reset for the new body.
        let saved_reassigned_local_slots = std::mem::take(&mut self.reassigned_local_slots);
        // Save function context. It's set by the caller (lower_letrec,
        // lower_define) before lower_expr so escape analysis can detect
        // self-tail-calls. We save it here and restore it for the
        // post-lowering escape analysis.
        let saved_function_binding = self.current_function_binding.take();
        let saved_function_params = self.current_function_params.take();
        // The self-recursive binding of THIS lambda body: the binding this lambda
        // captures as `CaptureKind::Recursive` (a same-binding self-edge, classified
        // in `hir/analyze/scopes.rs`). A reference to it inside the body — in value OR
        // call position — resolves to the executing closure via `LoadSelf` / a
        // self-call (`lower_var`), never a cell load. Read directly from the classified
        // fact, independent of whether the binding also keeps a cell for a sibling
        // (`needs_capture()`): the self-edge is resolved by the executing closure in
        // every case. Saved/restored so a nested lambda restores the enclosing value.
        let saved_self_binding = self.current_self_binding.take();
        let this_self_binding: Option<Binding> = captures.iter().find_map(|cap| {
            if let crate::hir::CaptureKind::Recursive { binding } = cap.kind {
                Some(binding)
            } else {
                None
            }
        });

        self.next_reg = 0;
        self.next_label = 1;
        self.discard_slot = None;
        // num_locals starts at 0; non-LBox params and let-bound vars
        // will increment it as they're allocated.
        // LBox params go into the env (not counted in num_locals for stack frame).
        self.current_func.num_locals = 0;
        self.current_func.num_captures = captures.len() as u16;
        self.in_lambda = true;
        self.num_captures = captures.len() as u16;
        self.num_local_params = 0;
        self.discard_slot = None;
        self.current_func.doc = doc;
        self.current_func.origin = origin;
        self.current_func.vararg_kind = vararg_kind.clone();
        self.current_func.num_params = params.len();

        // In a closure, the environment is laid out as:
        // [captured_vars..., parameters..., locally_defined_cells...]
        // So:
        // - Captured variables are at indices [0, num_captures)
        // - Parameters are at indices [num_captures, num_captures + num_params)

        // Bind captured variables to upvalue indices
        for (i, cap) in captures.iter().enumerate() {
            self.binding_to_slot.insert(cap.binding, i as u16);
            self.upvalue_bindings.insert(cap.binding);
        }

        // Build capture_params_mask and bind parameters.
        // LBox params → upvalues in the env (LoadCapture/StoreCapture).
        // Non-LBox params → locals (LoadLocal/StoreLocal), copied from env at entry.
        let mut capture_params_mask: u64 = 0;
        for (i, param) in params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();

            if needs_capture {
                if i < 64 {
                    capture_params_mask |= 1 << i;
                }
                // LBox param: lives in env as upvalue
                let upvalue_idx = self.num_captures + i as u16;
                self.binding_to_slot.insert(*param, upvalue_idx);
                self.upvalue_bindings.insert(*param);
            } else {
                // Non-LBox param: allocate a local slot.
                // We'll copy from env into this local at function entry.
                let slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.num_local_params += 1;
                self.binding_to_slot.insert(*param, slot);
                // NOT added to upvalue_bindings → uses LoadLocal/StoreLocal
            }
        }
        self.current_func.capture_params_mask = capture_params_mask;

        // Copy non-LBox params from env into their local slots.
        // The VM/host populates the env as [captures..., params...].
        // Non-LBox params are at env index (num_captures + i).
        for (i, param) in params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();
            if !needs_capture {
                let env_idx = self.num_captures + i as u16;
                let slot = *self.binding_to_slot.get(param).unwrap();
                let tmp = self.fresh_reg();
                self.emit(LirInstr::LoadCaptureRaw {
                    dst: tmp,
                    index: env_idx,
                });
                self.emit(LirInstr::StoreLocal { slot, src: tmp });
            }
        }

        self.current_func.num_local_params = self.num_local_params as usize;

        // Each param is an OWNED binding: the analysis gave it a placeholder
        // region in `call_result_regions` (see the Lambda arm of `regions.rs`).
        // Record `region_to_slot[param_r] = slot` so `emit_decrefs_for` can
        // release it at the param's true last use. `binding_to_slot` already
        // holds the right slot for each kind:
        //   - non-captured param → a LOCAL slot; released `LoadLocal` +
        //     `DecrefValueRegion` (drops the arg's runtime region).
        //   - captured (LBox) param → the UPVALUE/env index; its placeholder is
        //     in `cell_release_regions`, released `LoadCaptureRaw` +
        //     `DecrefCellRegion` (frees the env cell's own region).
        for param in params.iter() {
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

        // Restore function context for body lowering — needed by
        // emit_drop_dead_params to detect self-tail-calls.
        self.current_function_binding = saved_function_binding;
        self.current_function_params = saved_function_params.clone();
        // This body's self-reference resolves to the executing closure (value
        // path). Set before lowering the body; restored below.
        self.current_self_binding = this_self_binding;

        // Emit signal bound checks for each bounded parameter
        for pb in param_bounds {
            if let Some(&slot) = self.binding_to_slot.get(&pb.binding) {
                let src = self.fresh_reg();
                let is_upvalue = self.upvalue_bindings.contains(&pb.binding);
                if is_upvalue {
                    self.emit(LirInstr::LoadCapture {
                        dst: src,
                        index: slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal { dst: src, slot });
                }
                self.emit(LirInstr::CheckSignalBound {
                    src,
                    allowed_bits: pb.signal.bits,
                });
            }
        }

        // Lower body. (No tail-region suppression: ownership transfer
        // to the caller is carried by `IncrefValueRegion` at each
        // `Return` — the return-wrapping pass — and the callee's own
        // `DecrefRegion` fires normally at the `return_sites`-extended
        // decref_point. See `emit_decrefs_for`.)
        let result_reg = self.lower_expr(body)?;

        // Fallback release for UNUSED non-captured params. A used param's
        // placeholder region gets a `decref_point` (from its uses) and is
        // released by `emit_decrefs_for`; an unused param has no `region_data`
        // entry, so without this its moved-in arg would leak. The retain
        // ordering note in `lower_return` does not apply: an unused param is
        // never the return value, so no `IncrefValueRegion` precedes this.
        //
        // A param the body's tail call MOVES is released by the callee instead,
        // and that is why the release goes through the relocation wrapper rather
        // than straight out: when the body ends in a frame-replacing tail call
        // this emission point is dead, so the release is carried back ahead of
        // the `TailCall` for every param the call does not name — an unused param
        // is by definition not one of its arguments, but the exemption is what
        // keeps a param used ONLY as such an argument out of the hoist
        // (docs/impl/region/mechanism.md § "A release past a frame-replacing tail
        // call is not a release").
        //
        // The nil stamp is what makes the release SELF-CANCELLING, and with it
        // replicable into the arms of a branch the body ends in: whichever copy a
        // path reaches first blanks the slot, so a later copy loads `nil` and
        // no-ops. Blanking is free here — the param is used nowhere, so nothing
        // reads the slot again.
        let unused_params: Vec<(u16, crate::hir::region::Region)> = params
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
                s.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                s.emit(LirInstr::DecrefValueRegion { src: val_reg });
                if let Ok(nil_reg) = s.emit_const(crate::lir::LirConst::Nil) {
                    s.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                }
            });
        }

        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate inferred signal to LIR function
        self.current_func.signal = inferred_signal;

        self.current_function_binding = None;
        self.current_function_params = None;

        // Record this lambda's merged slots while `region_to_table` still holds
        // its slots (restored to the parent's below). Empty unless a builder-idiom
        // merge fired in this lambda — see `record_merged_slots`.
        self.record_merged_slots();

        let func = std::mem::replace(&mut self.current_func, saved_func);

        // Restore state
        self.current_block = saved_block;
        self.next_reg = saved_reg;
        self.next_label = saved_label;
        self.binding_to_slot = saved_bindings;
        self.in_lambda = saved_in_lambda;
        self.num_captures = saved_num_captures;
        self.num_local_params = saved_num_local_params;
        self.upvalue_bindings = saved_upvalue_bindings;
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
