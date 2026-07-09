//! Non-scoped mutating binding forms: `define` (local `def`) and `set`
//! (`lower_assign`). Grouped for the shared capture-cell store/reload and the
//! 1-slot-container drop-on-overwrite reference discipline.

use super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_define(
        &mut self,
        binding: Binding,
        value: &Hir,
    ) -> Result<Reg, String> {
        // Local define
        // Allocate the slot BEFORE lowering the value so that recursive
        // references can find the binding (like letrec)
        // The slot might already be allocated by the Begin pre-pass
        let slot = if let Some(&existing_slot) = self.binding_to_slot.get(&binding) {
            existing_slot
        } else {
            self.allocate_slot(binding)
        };

        // Check if this binding needs to be wrapped in a cell
        let needs_capture = self.arena.get(binding).needs_capture();

        // Only LBox-wrapped locals need upvalue treatment inside lambdas
        if self.in_lambda && needs_capture {
            self.upvalue_bindings.insert(binding);
        }

        // Set function context for lambdas so that
        // body_escapes_heap_values can detect self-tail-calls.
        if let HirKind::Lambda { params, .. } = &value.kind {
            self.current_function_binding = Some(binding);
            self.current_function_params = Some(params.clone());
        }

        // Record the slot BEFORE lowering the value so
        // `emit_decrefs_for(value.id)` inside `lower_expr` can find
        // it (matches `lower_let`'s ordering).
        //
        // EXCEPTION — a top-level captured AND reassigned binding: its slot
        // holds the `MakeCaptureCell` whose content a later reassignment
        // repoints, so routing the init's region through this slot makes its
        // decref reload the cell and (via `result_region_of`, which unwraps a
        // capture cell) free whatever the cell holds at the decref's RUNTIME
        // firing point — a different, live value (the capture-cell reassign UAF;
        // region-capture-cell-reassign-uaf.lisp). Skip the routing and drop the
        // init's alloc reference off its register below. A captured binding
        // never reassigned keeps the routing (stable cell content).
        let captured_reassigned = self
            .region_info
            .captured_reassigned_bindings
            .contains(&binding);
        if !captured_reassigned {
            self.record_region_slot(value.id, slot);
        }

        // Now lower the value (which can reference the binding)
        let value_reg = self.lower_expr(value)?;
        self.current_function_binding = None;
        self.current_function_params = None;

        // Self-recursive `def` nested in a lambda: lowering `value` above ran
        // `lower_lambda_expr`, which recorded this binding in `self_recursive_bindings`.
        // Its cell-free closure region demises at the binding's last use — the
        // func-load of the `(loop …)` recursive call — so the lowerer would emit a LIVE
        // `DecrefRegion` right before that call, freeing the closure out from under its
        // own re-entry (the executing-closure re-dispatch then reads a recycled page).
        // The `letrec` path avoids this by landing that decref at the letrec scope end
        // — dead code past the body's frame-replacing `TailCall`, supplied once by the
        // adopt. Mirror it for `def`: SUPPRESS the closure region's `DecrefRegion`
        // (`suppressed_self_regions`) and STRAND the binding (`stranded_self_bindings`)
        // so a tail call to it adopts the region — the sole, once-only release.
        // Cell-free self-recursion only (see the `lower_letrec` twin): a
        // sibling-captured (`needs_capture`) self-recursive binding is held by a
        // cell, so its region is released by the cell's cascade, not this
        // suppress-and-strand adopt — stranding it double-frees under the live cell.
        if self.self_recursive_bindings.contains(&binding)
            && !self.arena.get(binding).needs_capture()
        {
            if let Some(&closure_region) = self.region_info.alloc_region.get(&value.id) {
                self.suppressed_self_regions.insert(closure_region);
            }
            self.stranded_self_bindings.insert(binding);
        }

        // Seed immutable_values for constant definitions
        self.try_seed_immutable(binding, value);

        if self.in_lambda && needs_capture {
            // Captured local → a `populate_env` env cell (StoreCapture into a
            // pre-allocated cell, no compiled MakeCaptureCell). Record its
            // env-cell placeholder so `emit_decrefs_for` releases the cell at
            // this binding's last use (`LoadCaptureRaw` + `DecrefCellRegion`).
            // A binding captured only by its own self-edge is cell-free
            // (`needs_capture() == false`) and never reaches this branch; only a
            // binding a sibling captures does, and it owns a genuine cell to release.
            self.record_env_cell_release_slot(binding, slot);
            self.emit(LirInstr::StoreCapture {
                index: slot,
                src: value_reg,
            });
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadCapture {
                dst: result,
                index: slot,
            });
            Ok(result)
        } else if self.in_lambda {
            self.emit_binding_store(slot, value_reg);
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: result, slot });
            Ok(result)
        } else if needs_capture {
            // The cell was already created in the Begin pre-pass. Store the init
            // into it; if the binding is reassigned, drop the init's alloc
            // reference off its own register (NOT via the binding slot, which
            // holds the cell — see the suppressed `record_region_slot` above).
            self.store_captured_cell_init(binding, slot, value_reg, value, captured_reassigned);
            // Reload from cell
            let cell_reg2 = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: cell_reg2,
                slot,
            });
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadCaptureCell {
                dst: result,
                cell: cell_reg2,
            });
            Ok(result)
        } else {
            self.emit_binding_store(slot, value_reg);
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: result, slot });
            Ok(result)
        }
    }

    pub(in crate::lir::lower) fn lower_assign(
        &mut self,
        target: &Binding,
        value: &Hir,
    ) -> Result<Reg, String> {
        // Evict stale constant — this binding is being mutated.
        self.immutable_values.remove(target);

        let value_reg = self.lower_expr(value)?;

        // Check if this binding needs cell update
        let needs_capture = self.arena.get(*target).needs_capture();

        // Check if this is an upvalue (capture or parameter) or a local
        let is_upvalue = self.upvalue_bindings.contains(target);

        if let Some(&slot) = self.binding_to_slot.get(target) {
            if self.in_lambda && is_upvalue && needs_capture {
                // For LBox upvalues, use StoreCapture (updates cell) + LoadCapture (unwraps)
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: value_reg,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadCapture {
                    dst: result,
                    index: slot,
                });
                Ok(result)
            } else if needs_capture {
                // For local variables that need cells, load the cell and update it
                let cell_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg,
                    slot,
                });
                self.emit(LirInstr::StoreCaptureCell {
                    cell: cell_reg,
                    value: value_reg,
                });
                let cell_reg2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg2,
                    slot,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadCaptureCell {
                    dst: result,
                    cell: cell_reg2,
                });
                Ok(result)
            } else if self
                .current_hir_id
                .is_some_and(|id| self.region_info.drop_on_overwrite_sites.contains(&id))
            {
                // A reassigned, sole-held, top-level (file-letrec) mutable is a
                // 1-slot mutable container (docs/impl/region/bindings.md Rule 5). The cell
                // owns its current content: increment the NEW value's region (the
                // cell now holds a reference — the root pin), and decrement the
                // displaced OLD value's region (the cell no longer holds it; its
                // demise is HERE, at the overwrite, where the slot still points at
                // it). The new value's own producer-temp decref still fires at its
                // last use, dropping the alloc reference and leaving exactly the
                // cell's reference — so each value's region reaches 0 precisely
                // when the cell stops holding it: at the next overwrite, or, for
                // the final (never-overwritten) value, at the file-letrec frame's
                // teardown. No static last-use decref is placed against the cell —
                // a cell has no single value-lifetime to name. The init-region
                // decref (which would mis-target the slot's CURRENT value) is
                // suppressed by `analyze_regions_with`; the init value is released
                // by the first overwrite's old-decref below.
                let old_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal { dst: old_reg, slot });
                self.emit(LirInstr::StoreLocal {
                    slot,
                    src: value_reg,
                });
                // Pin the new content as the cell's reference — UNLESS the cell
                // already owns the producer's reference outright. A MODULE-SCOPE
                // 1-slot container's value regions have their ordinary decref
                // suppressed (`analyze_regions_with`), donating the producer's single
                // reference to the cell; the drop-on-overwrite below is that
                // reference's sole release, so an incref-on-store here would be
                // unbalanced (born + store − overwrite = +1), holding every
                // displaced prior to frame teardown (docs/impl/region/bindings.md
                // "Reassigned mutable bindings are 1-slot containers"). These sites
                // are marked `donated_overwrite_sites`. A FN-LOCAL container instead
                // KEEPS the assign-value decref (its scope-exit demise), so it must
                // take its own counted reference here, balanced by drop-on-overwrite.
                let donated = self
                    .current_hir_id
                    .is_some_and(|id| self.region_info.donated_overwrite_sites.contains(&id));
                if !donated {
                    // Transform 1 (docs/impl/region/mechanism.md § "Compile-time region
                    // selection (coalescing)"): a fresh local allocation whose region
                    // is a known slot pins slot-resolved (`IncrefRegion`, guarded by
                    // the equivalence oracle), mirroring `lower_return`; otherwise
                    // value-resolved — the dynamic boundary `coalescible_region`
                    // enforces.
                    let coalesced = self.coalescible_region(value);
                    super::super::rcstats::record_reassign_store(coalesced.is_some());
                    match coalesced {
                        Some(region_id) => {
                            #[cfg(debug_assertions)]
                            self.emit(LirInstr::AssertRegionMatches {
                                region_id,
                                src: value_reg,
                            });
                            self.emit(LirInstr::IncrefRegion { region_id });
                        }
                        None => self.emit(LirInstr::IncrefValueRegion { src: value_reg }),
                    }
                }
                // Drop the cell's reference to the displaced content (the displaced
                // 1-slot content is a runtime fact — stays value-resolved, the
                // dynamic boundary).
                self.emit(LirInstr::DecrefValueRegion { src: old_reg });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadLocal { dst: result, slot });
                Ok(result)
            } else {
                self.emit(LirInstr::StoreLocal {
                    slot,
                    src: value_reg,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadLocal { dst: result, slot });
                Ok(result)
            }
        } else {
            Err(format!("Unknown binding: {:?}", target))
        }
    }
}
