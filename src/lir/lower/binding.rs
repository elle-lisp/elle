//! Binding-related lowering: let, letrec, define, set

use super::*;
use crate::hir::PatternKey;

mod destructure;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_let(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };
        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        // Allocate slots and lower initializers
        for (binding, init) in bindings {
            self.try_seed_immutable(*binding, init);

            // Allocate the binding's slot BEFORE lowering the init,
            // and register `region_to_slot[r] = slot` so that the
            // `emit_decrefs_for(init.id)` call inside `lower_expr` —
            // which fires for unused bindings whose `decref_point` is the
            // init's own HirId — can find the slot it needs to load
            // the value from for `DecrefValueRegion`. Without this
            // pre-allocation, an unused let-bound Call result leaks
            // because the slot only exists after `lower_expr` returns.
            //
            // `allocate_slot` stamps the slot with `StoreLocal(slot,
            // nil)`. When the binding is unused, the init region's
            // `decref_point` is the init's own HirId, so `lower_expr`'s
            // trailing `emit_decrefs_for(init.id)` would reload the slot and
            // decref it — but the slot still holds that stamped `nil`, since
            // the init value is not stored until *after* `lower_expr`
            // returns. The decref would hit `nil` and the real value's region
            // would leak. Defer the init node's decrefs, store the value,
            // then emit them against the now-populated slot.
            let slot = self.allocate_slot(*binding);
            self.record_region_slot(init.id, slot);
            self.deferred_decref_points.insert(init.id);
            let init_reg = self.lower_expr(init)?;
            self.emit_counted_cell_read_retain(init.id, init_reg);
            let needs_capture = self.arena.get(*binding).needs_capture();

            if self.in_lambda && needs_capture {
                self.upvalue_bindings.insert(*binding);
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else if self.in_lambda {
                self.emit_binding_store(slot, init_reg);
            } else {
                if needs_capture {
                    // One region PER cell, looked up by binding — multiple
                    // captured bindings in one Let must not share this node's
                    // single slot (the shared-slot capture-cell leak;
                    // docs/impl/region-model.md, "one allocation execution per slot
                    // between drops").
                    let region = self.cell_region_for(*binding);
                    let cell_reg = self.fresh_reg();
                    self.emit_alloc_in(region, |region| LirInstr::MakeCaptureCell {
                        region,
                        dst: cell_reg,
                        value: init_reg,
                    });
                    self.emit_binding_store(slot, cell_reg);
                } else {
                    self.emit_binding_store(slot, init_reg);
                }
            }

            // The slot now holds the init value (or its capture cell), so the
            // deferred `DecrefValueRegion` reloads the right value.
            self.deferred_decref_points.remove(&init.id);
            self.emit_decrefs_for(init.id, None);
        }
        // For tail calls in scoped lets, emit FreeRegion before the
        // tail call via the pending mechanism.
        let tail_scoped = region_id.is_some() && Self::body_is_tail_call(body);
        if let Some(rid) = region_id {
            if tail_scoped {
                self.pending_free_regions.push(rid);
            }
        }
        let result = self.lower_expr(body)?;
        // Pop region from active stack BEFORE deciding whether to emit
        // FreeRegion. If the region is still in the stack, an outer scope
        // also uses it and will emit its own FreeRegion — emitting one here
        // would double-decref and free the region prematurely.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }
        if tail_scoped {
            self.pending_free_regions.pop();
        }
        // Region-demise `DecrefRegion` is emitted by `lower_expr`'s
        // `emit_decrefs_for` at each region's `decref_point` HirId.
        let _ = region_id;
        Ok(result)
    }

    pub(super) fn lower_letrec(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };
        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        // First allocate all slots with nil (or cells containing nil)
        for (binding, init) in bindings.iter() {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            let bi = self.arena.get(*binding);
            let needs_capture = bi.needs_capture();
            // A COMPILED forward cell (every captured binding at top level; the
            // immutable lambda-initialized shape inside a lambda — the
            // closure-cycle merge's static-slot cells) lives in the binding's
            // own stack slot; every other in-lambda captured binding keeps the
            // env-cell route (StoreCapture into the `populate_env` cell).
            let compiled_cell = bi
                .letrec_compiled_cell(matches!(init.kind, HirKind::Lambda { .. }), self.in_lambda);
            let slot = self
                .allocate_slot_routed(*binding, self.in_lambda && needs_capture && !compiled_cell);

            if compiled_cell {
                // One region PER cell — a letrec pre-allocates one cell per
                // captured binding, and emitting them all against the letrec's
                // single slot leaks every cell but the last (the shared-slot
                // capture-cell leak; docs/impl/region-model.md, "one allocation
                // execution per slot between drops").
                let region = self.cell_region_for(*binding);
                let cell_reg = self.fresh_reg();
                self.emit_alloc_in(region, |region| LirInstr::MakeCaptureCell {
                    region,
                    dst: cell_reg,
                    value: nil_reg,
                });
                self.emit_binding_store(slot, cell_reg);
            } else if self.in_lambda && needs_capture {
                self.upvalue_bindings.insert(*binding);
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: nil_reg,
                });
            } else {
                self.emit_binding_store(slot, nil_reg);
            }
        }
        // Then initialize
        for (binding, init) in bindings.iter() {
            // Set function context for lambdas so that
            // body_escapes_heap_values can detect self-tail-calls.
            if let HirKind::Lambda { params, .. } = &init.kind {
                self.current_function_binding = Some(*binding);
                self.current_function_params = Some(params.clone());
            }
            let slot = self.binding_to_slot[binding];
            // Record the slot BEFORE lowering the init so
            // `emit_decrefs_for(init.id)` inside `lower_expr` can
            // find it (matches `lower_let`'s ordering).
            //
            // EXCEPTION — a top-level captured AND reassigned binding's slot
            // holds the `MakeCaptureCell` whose content a later reassignment
            // repoints; routing the init's region through this slot makes its
            // decref free a different, live value (the capture-cell reassign
            // UAF). Skip the routing and drop the init's alloc reference off its
            // register below via `store_captured_cell_init`. (A captured binding
            // that is never reassigned keeps the routing — the cell content is
            // stable, so the unwrap always names this init value.)
            let captured_reassigned = self
                .region_info
                .captured_reassigned_bindings
                .contains(binding);
            if !captured_reassigned {
                self.record_region_slot(init.id, slot);
            }
            // Defer the init node's region releases until after the value is
            // stored (mirrors `lower_let`). Without this, an init region whose
            // `decref_point` is the init's own node — an UNUSED captured
            // binding's closure, e.g. a shadowed duplicate definition — has
            // its `DecrefRegion` emitted between `MakeClosure` and the cell
            // store: the closure is freed before `UpdateCapture` increfs it,
            // and the cell holds a dangling value whose free-time scan
            // misattributes the pages to their next tenant (the teardown
            // phantom-decref panic / double-free).
            self.deferred_decref_points.insert(init.id);
            let init_reg = self.lower_expr(init)?;
            self.current_function_binding = None;
            self.current_function_params = None;

            // Seed immutable_values after init so subsequent bindings
            // and the body can use LoadConst for this constant.
            // Skip nil inits — letrec destructure leaves are initialized
            // to nil here and later updated by a Destructure node in the body.
            // For non-nil inits, evict any stale value first (file-scope
            // duplicate names may reuse the same Binding identity).
            if !matches!(init.kind, HirKind::Nil) {
                self.immutable_values.remove(binding);
                self.try_seed_immutable(*binding, init);
            }

            // A compiled-cell binding stores its init into the pre-allocated
            // MakeCaptureCell (its slot holds the CELL); an env-celled upvalue
            // stores through the populate_env cell; everything else is a plain
            // slot store.
            let compiled_cell = self
                .arena
                .get(*binding)
                .letrec_compiled_cell(matches!(init.kind, HirKind::Lambda { .. }), self.in_lambda);
            let is_upvalue = self.upvalue_bindings.contains(binding);

            if compiled_cell {
                self.store_captured_cell_init(slot, init_reg, init, captured_reassigned);
            } else if self.in_lambda && is_upvalue {
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else {
                self.emit_binding_store(slot, init_reg);
            }
            // The slot/cell now holds the init value; releases deferred above
            // fire here, after the store's incref (see `lower_let`).
            self.deferred_decref_points.remove(&init.id);
            self.emit_decrefs_for(init.id, None);
        }
        let tail_scoped = region_id.is_some() && Self::body_is_tail_call(body);
        if let Some(rid) = region_id {
            if tail_scoped {
                self.pending_free_regions.push(rid);
            }
        }
        // A self-recursive binding of THIS letrec is cell-free, but its closure region
        // lives through the recursion and its scope-end `DecrefRegion` lands at this
        // letrec's scope end. When the body is a tail call that scope end is dead code
        // past the `TailCall`, so the decref never runs and the region leaks. Mark such
        // bindings stranded BEFORE lowering the body, so a tail call to one (`(loop k)`
        // here, or its own `(loop …)` self-call) adopts the region — the runtime's
        // `adopted_closures` release supplies the stranded decref exactly once. Gating
        // on `body_is_tail_call` (not `tail_scoped`, which also requires a scope region)
        // is deliberate: the stranding is a property of the body. Without the tail-call
        // gate a non-tail letrec body would adopt a binding whose decref fires live — a
        // double-free.
        if Self::body_is_tail_call(body) {
            for (b, _) in bindings.iter() {
                // Cell-free self-recursion only. A self-recursive binding that is
                // ALSO captured by a sibling (`needs_capture`) is held by a letrec
                // cell whose lifetime outlives this tail-call activation; its
                // closure region is released by the cell's cascade, so a tail-call
                // adopt would decref it a SECOND time and free it under the still-live
                // cell (the scheduler's `handle-fiber-after-resume` — self-recursive
                // AND sibling-captured — freed under its forward cell; a stale
                // `tail_callee_adopt_region` deref of the next self-call).
                // docs/impl/selfrec.md: the cell-free case is exactly the one the
                // self-edge leaves uncaptured. Pinned by
                // tests/elle/region-selfrec-captured-tail-adopt.lisp.
                if self.self_recursive_bindings.contains(b) && !self.arena.get(*b).needs_capture() {
                    self.stranded_self_bindings.insert(*b);
                }
            }
        }
        // A letrec body that TAIL-CALLS a closure-cycle merge member strands the
        // merged arena's binding-scope DecrefRegion as dead code past the
        // frame-replacing TailCall; mark those callees so the body's tail call
        // adopts the merged region (`tail_callee_adopts`) — the runtime then
        // releases it exactly once at the recursion's normal completion. Scanned
        // over the letrec BODY only and never through nested lambdas: a nested
        // closure's tail call completes inside its own activation, before later
        // uses of the arena, so adopting there would free it early (the
        // non-upvalue guard in `tail_callee_adopts` is the second half of that
        // exclusion). Marked BEFORE lowering the body so the body's own call
        // sites see it; the init lambdas were lowered above, so an interior
        // sibling rotation (`ev` tail-calling `od`) is never marked. The merge
        // admits a cycle only when every body tail call targets a member
        // (`compute_closure_cycle_merges`), so no admitted cycle is left with an
        // unmarked stranding tail path.
        {
            let mut tail_callees: Vec<Binding> = Vec::new();
            Self::collect_body_tail_callees(body, &mut tail_callees);
            for b in tail_callees {
                if self
                    .region_info
                    .binding_source_regions
                    .get(&b)
                    .is_some_and(|rs| {
                        rs.iter()
                            .any(|r| self.region_info.closure_cycle_members.contains(r))
                    })
                {
                    self.stranded_cycle_bindings.insert(b);
                }
            }
        }
        let result = self.lower_expr(body)?;
        // Pop first, then check if region is still in stack (shared with
        // outer scope). See lower_let for rationale.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }
        if tail_scoped {
            self.pending_free_regions.pop();
        }
        // Region-demise DecrefRegion emission is in `lower_expr`.
        let _ = region_id;
        Ok(result)
    }

    /// Collect the target bindings of every tail call in a letrec BODY: each
    /// `Call { is_tail: true }` whose callee (through the `DerefCell` wrapper
    /// `functionalize` adds around a needs-capture binding read) is a `Var`.
    /// Never descends into a `Lambda` — a nested closure's tail calls run in
    /// that closure's own activation, not the letrec's, so they neither strand
    /// nor may adopt the letrec's merged arena.
    fn collect_body_tail_callees(hir: &Hir, out: &mut Vec<Binding>) {
        if matches!(hir.kind, HirKind::Lambda { .. }) {
            return;
        }
        if let HirKind::Call {
            func,
            is_tail: true,
            ..
        } = &hir.kind
        {
            let callee = match &func.kind {
                HirKind::DerefCell { cell } => cell,
                _ => func,
            };
            if let HirKind::Var(b) = &callee.kind {
                out.push(*b);
            }
        }
        hir.for_each_child(|c| Self::collect_body_tail_callees(c, out));
    }

    pub(super) fn lower_define(&mut self, binding: Binding, value: &Hir) -> Result<Reg, String> {
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
            self.store_captured_cell_init(slot, value_reg, value, captured_reassigned);
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

    pub(super) fn lower_assign(&mut self, target: &Binding, value: &Hir) -> Result<Reg, String> {
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
                // 1-slot mutable container (docs/impl/region-bindings.md Rule 5). The cell
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
                // displaced prior to frame teardown (docs/impl/region-bindings.md
                // "Reassigned mutable bindings are 1-slot containers"). These sites
                // are marked `donated_overwrite_sites`. A FN-LOCAL container instead
                // KEEPS the assign-value decref (its scope-exit demise), so it must
                // take its own counted reference here, balanced by drop-on-overwrite.
                let donated = self
                    .current_hir_id
                    .is_some_and(|id| self.region_info.donated_overwrite_sites.contains(&id));
                if !donated {
                    // Transform 1 (docs/impl/region-rules.md § "Compile-time region
                    // selection (coalescing)"): a fresh local allocation whose region
                    // is a known slot pins slot-resolved (`IncrefRegion`, guarded by
                    // the equivalence oracle), mirroring `lower_return`; otherwise
                    // value-resolved — the dynamic boundary `coalescible_region`
                    // enforces.
                    let coalesced = self.coalescible_region(value);
                    super::rcstats::record_reassign_store(coalesced.is_some());
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

    /// Lower a Destructure node: evaluate the value, then destructure into bindings.
    /// Returns a nil register (destructuring is a statement, not an expression).
    /// `strict`: if true, missing/wrong-type values signal error; if false, produce nil.
    pub(super) fn lower_destructure_expr(
        &mut self,
        pattern: &HirPattern,
        value: &Hir,
        strict: bool,
        _span: &Span,
    ) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;
        self.lower_destructure(pattern, value_reg, strict)?;
        // Destructure produces nil as its expression value
        self.emit_const(LirConst::Nil)
    }

    /// Lower MakeCell — currently transparent (just lowers the inner value).
    ///
    /// **Double-handling contract:** Both functionalize AND the lowerer handle
    /// cells. Functionalize inserts explicit MakeCell/DerefCell/SetCell nodes;
    /// the lowerer's lower_let/lower_letrec/lower_define independently wrap
    /// needs_capture bindings in cells. The transparent delegation here works
    /// because both sides agree on which bindings need cells (via
    /// `needs_capture()`). Phase 3 will remove the lowerer's implicit cell
    /// creation and make these methods emit real cell instructions.
    pub(super) fn lower_make_cell(&mut self, value: &Hir) -> Result<Reg, String> {
        self.lower_expr(value)
    }

    /// Lower DerefCell — currently transparent (just lowers the inner cell expr).
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_var` already unwraps cells for needs_capture bindings, so
    /// DerefCell's child (a Var) produces the unwrapped value directly.
    pub(super) fn lower_deref_cell(&mut self, cell: &Hir) -> Result<Reg, String> {
        self.lower_expr(cell)
    }

    /// Lower SetCell — delegates to lower_assign since the lowerer already
    /// handles cell stores. The cell child must be a Var.
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_assign` already stores through cells for needs_capture bindings.
    pub(super) fn lower_set_cell(&mut self, cell: &Hir, value: &Hir) -> Result<Reg, String> {
        if let HirKind::Var(binding) = &cell.kind {
            self.lower_assign(binding, value)
        } else {
            Err("SetCell: cell must be a Var".to_string())
        }
    }

    /// Store a value into a binding, consuming it from the stack.
    /// Used by lower_destructure.
    fn lower_bind_value(&mut self, binding: Binding, value_reg: Reg) -> Result<Reg, String> {
        // Evict stale constant — this binding is being (re-)assigned
        // (e.g., file-scope destructure reusing an earlier binding).
        self.immutable_values.remove(&binding);
        // Allocate slot if not already done (Begin pre-pass may have done it)
        let slot = if let Some(&existing_slot) = self.binding_to_slot.get(&binding) {
            existing_slot
        } else {
            self.allocate_slot(binding)
        };

        let needs_capture = self.arena.get(binding).needs_capture();

        if self.in_lambda && needs_capture {
            self.upvalue_bindings.insert(binding);
            self.emit(LirInstr::StoreCapture {
                index: slot,
                src: value_reg,
            });
        } else if self.in_lambda {
            self.emit_binding_store(slot, value_reg);
        } else if needs_capture {
            // cell was already created in Begin pre-pass
            let cell_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: cell_reg,
                slot,
            });
            self.emit(LirInstr::StoreCaptureCell {
                cell: cell_reg,
                value: value_reg,
            });
        } else {
            self.emit_binding_store(slot, value_reg);
        }
        Ok(value_reg)
    }
}
