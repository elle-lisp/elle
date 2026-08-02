//! Recursive/scoped binding forms: `let` and `letrec`.
//!
//! These share the region-scope, capture-cell, and tail-call stranding
//! machinery — kept together so the parallel `lower_let`/`lower_letrec`
//! ordering (slot-before-init, deferred decrefs) reads side by side.

use super::*;

/// The tail call inside ANF's canonical wrap `(let [t (f …)] (return t))`, whose
/// `Return` mint therefore covers the call's result. `None` for any other `let`:
/// a multi-binding one, a body that is not this binding's `Return`, or a
/// non-tail init (an ordinary call's own return convention already balances).
fn anf_wrapped_return_minted_call(bindings: &[(Binding, Hir)], body: &Hir) -> Option<HirId> {
    let [(binding, init)] = bindings else {
        return None;
    };
    let HirKind::Return { value } = &body.kind else {
        return None;
    };
    let HirKind::Var(returned) = &value.kind else {
        return None;
    };
    (returned == binding && matches!(init.kind, HirKind::Call { is_tail: true, .. }))
        .then_some(init.id)
}

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_let(
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
        // ANF's canonical wrap of a tail call — `(let [t (f …)] (return t))`,
        // built when the tail call sits in a `begin`/`if`/`cond`/`match` arm —
        // names the result, so `lower_return` mints the caller's reference here
        // and this binding's `decref_point` drops the frame's own. Record the
        // call so its post-`TailCall` fall-through retain stands down: exactly
        // one return mint per returned value (docs/impl/region/mechanism.md
        // § "The return mint is emitted exactly once"). This is the only site
        // that sees the wrap — ANF builds no other binding form for it.
        if let Some(call_id) = anf_wrapped_return_minted_call(bindings, body) {
            self.return_minted_calls.insert(call_id);
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
            self.record_region_slot(init.id, self.value_slot_for(*binding, slot));
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
                    // docs/impl/region/model.md, "one allocation execution per slot
                    // between drops").
                    let region = self.cell_region_for(*binding);
                    let cell_reg = self.fresh_reg();
                    self.emit_alloc_in(region, |region| LirInstr::MakeCaptureCell {
                        region,
                        dst: cell_reg,
                        value: init_reg,
                    });
                    // `cell ⊇ content`: adopt the init value into the cell's own region if
                    // the forest admitted it. A `lower_let` compiled cell is always a
                    // re-storable `@`-mutable local (a `let` binding is never prebound), so
                    // this is a no-op today (gate D refuses re-storable content); kept for
                    // uniformity with the letrec/define cell-store sites.
                    self.maybe_emit_cell_content_adopt(*binding, cell_reg, init_reg);
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
        if tail_scoped {
            self.pending_free_regions.pop();
        }
        // Region-demise `DecrefRegion` is emitted by `lower_expr`'s
        // `emit_decrefs_for` at each region's `decref_point` HirId.
        let _ = region_id;
        Ok(result)
    }

    pub(in crate::lir::lower) fn lower_letrec(
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
                // capture-cell leak; docs/impl/region/model.md, "one allocation
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
            // EXCEPTION — a captured AND reassigned binding's slot holds the
            // `MakeCaptureCell` whose content a later reassignment repoints —
            // wherever that reassignment sits, a sibling form or a closure the
            // defining scope encloses; routing the init's region through this slot
            // makes its decref free a different, live value (the capture-cell
            // reassign UAF). Skip the routing and drop the init's alloc reference off its
            // register below via `store_captured_cell_init`. (A captured binding
            // that is never reassigned keeps the routing — the cell content is
            // stable, so the unwrap always names this init value.)
            let captured_reassigned = self
                .region_info
                .captured_reassigned_bindings
                .contains(binding);
            if !captured_reassigned {
                // A COMPILED forward cell took a stack slot even when captured
                // in a lambda (`allocate_slot_routed(.., !compiled_cell)` in the
                // pre-pass above), so the space follows that same condition
                // rather than `value_slot_for`'s capture test.
                let celled = self.arena.get(*binding).letrec_compiled_cell(
                    matches!(init.kind, HirKind::Lambda { .. }),
                    self.in_lambda,
                );
                let space = if self.in_lambda && self.arena.get(*binding).needs_capture() && !celled
                {
                    super::super::ValueSlot::Env(slot)
                } else {
                    super::super::ValueSlot::Local(slot)
                };
                self.record_region_slot(init.id, space);
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
                self.store_captured_cell_init(*binding, slot, init_reg, init, captured_reassigned);
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
        // here, or its own `(loop …)` self-call) defers the region's release — the
        // runtime's
        // `deferred_releases` supplies the stranded decref exactly once. Gating
        // on `body_is_tail_call` (not `tail_scoped`, which also requires a scope region)
        // is deliberate: the stranding is a property of the body. Without the tail-call
        // gate a non-tail letrec body would defer a binding whose decref fires live — a
        // double-free.
        if Self::body_is_tail_call(body) {
            for (b, _) in bindings.iter() {
                // Cell-free self-recursion only. A self-recursive binding that is
                // ALSO captured by a sibling (`needs_capture`) is held by a letrec
                // cell whose lifetime outlives this tail-call activation; its
                // closure region is released by the cell's cascade, so a tail-call
                // deferred release would decref it a SECOND time and free it under the still-live
                // cell (the scheduler's `handle-fiber-after-resume` — self-recursive
                // AND sibling-captured — freed under its forward cell; a stale
                // `tail_callee_release_region` deref of the next self-call).
                // docs/impl/selfrec.md: the cell-free case is exactly the one the
                // self-edge leaves uncaptured. Pinned by
                // tests/elle/region-selfrec-captured-tail-release.lisp.
                if self.self_recursive_bindings.contains(b) && !self.arena.get(*b).needs_capture() {
                    self.stranded_self_bindings.insert(*b);
                }
            }
        }
        // A letrec body that TAIL-CALLS a closure-cycle merge MEMBER strands the
        // merged arena's binding-scope DecrefRegion as dead code past the
        // frame-replacing TailCall; mark those callees so the body's tail call
        // defers the merged region's release (`tail_callee_defers_release`) — the runtime then
        // releases it exactly once at the recursion's normal completion. Scanned
        // over the letrec BODY only and never through nested lambdas: a nested
        // closure's tail call completes inside its own activation, before later
        // uses of the arena, so deferring there would free it early (the
        // non-upvalue guard in `tail_callee_defers_release` is the second half of that
        // exclusion). Marked BEFORE lowering the body so the body's own call
        // sites see it; the init lambdas were lowered above, so an interior
        // sibling rotation (`ev` tail-calling `od`) is never marked. A NON-member
        // body tail (a native / redefined operator / foreign fn) instead rides the
        // explicit `TailCall::deferred_release_slot` (keyed by HirId in
        // `RegionInfo::cycle_tail_release`), NOT this binding-keyed marking — the two
        // channels are disjoint, so every admitted cycle's stranding tail paths are
        // covered exactly once (`compute_closure_cycle_merges`).
        //
        // The same body tail call strands a second, disjoint release: the callee's
        // OWN closure region, where the callee is a member of THIS letrec whose
        // uses span it — a member a sibling captures is allocated per call and its
        // demise lands at this scope end rather than at the call node, so the
        // dies-here reading never claims it. The exemption keeps that release in
        // the dead block on the premise that the new activation takes it over, so
        // the deferral has to reach a release placed here (mechanism.md § "What the
        // exemption keeps, a channel must still run"). Two exclusions keep the
        // three channels naming disjoint regions: a `closure_cycle_members` region
        // is the merge's to release, and a SUPPRESSED release belongs to the store
        // or capture-adopt path that claimed the region — deferring either
        // decrements a count this frame never raised.
        {
            let mut tail_callees: Vec<Binding> = Vec::new();
            Self::collect_body_tail_callees(body, &mut tail_callees);
            let scope_end_releases: Vec<crate::hir::region::Region> = self
                .decrefs_by_decref_point
                .get(&hir_id)
                .cloned()
                .unwrap_or_default();
            for b in tail_callees {
                let Some(sources) = self.region_info.binding_source_regions.get(&b) else {
                    continue;
                };
                if sources
                    .iter()
                    .any(|r| self.region_info.closure_cycle_members.contains(r))
                {
                    self.stranded_cycle_bindings.insert(b);
                    continue;
                }
                if sources.iter().any(|r| {
                    scope_end_releases.contains(r)
                        && !self.region_info.suppressed_decref_regions.contains(r)
                }) {
                    self.stranded_member_bindings.insert(b);
                }
            }
        }
        let result = self.lower_expr(body)?;
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
    /// nor may free the letrec's merged arena.
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
}
