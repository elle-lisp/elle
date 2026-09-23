// audited: 2026-09-23
//! Region releases: what the lowerer emits at each region's `decref_point`, routed by the region's class.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/rules.md
//! docs/impl/region/unwind.md
//!
//! Split by what a release names, all methods hanging off the shared
//! `impl<'a> Lowerer<'a>`:
//!   - this file: the per-node sequencing, the per-region routing, the
//!     value route's refusals, and the slot-resolved `DecrefRegion`
//!   - `routes`: the value, env-cell, 1-slot-container and co-owned-group routes
//!   - `arms`:   the per-arm compensation a branch owes

use super::*;

mod arms;
mod routes;

impl<'a> Lowerer<'a> {
    /// Emit region-demise instructions at this node's `decref_point`:
    ///
    /// - Call-result regions with a known binding slot: emit
    ///   `LoadLocal slot` + `DecrefValueRegion` so the decref uses
    ///   the *runtime* region of the actual returned value, not the
    ///   compile-time `call_r` placeholder.
    /// - Call-result regions without a slot whose decref_point is the
    ///   call node ITSELF (`alloc_region[hir_id] == r` — a discarded
    ///   result): release by value off `result_reg`, the freshly-
    ///   lowered result (docs/impl/region/rules.md Rule 2, "discarded result").
    /// - All other regions: emit `DecrefRegion(rid)` — the
    ///   compile-time region ID matches the runtime region (alloc
    ///   opcodes used the compile-time ID as the bytecode operand).
    ///
    /// `result_reg` is the just-lowered value of the node at `hir_id`
    /// when called from `lower_expr` (None from the binding-site
    /// callers, whose deferred decrefs never target their own node's
    /// result region).
    pub(super) fn emit_decrefs_for(&mut self, hir_id: HirId, result_reg: Option<Reg>) {
        // O(1) lookup into the decref_point-indexed map built in
        // `with_region_info`.
        //
        // Ownership transfer to the caller is carried by `IncrefValueRegion`
        // (emitted at each `Return`), not by withholding the callee's
        // `DecrefRegion`. A freshly-
        // allocated tail region is retained (+1 for the caller) then
        // decreffed normally here (the `return_sites` decref_point extension
        // orders this decref after the retain). A direct tail call's
        // result is unbound, so it has no release at all (pure transfer);
        // a let-bound tail-call result is retained and released, netting a
        // clean +1 transfer.
        let regions: Vec<crate::hir::region::Region> = self
            .decrefs_by_decref_point
            .get(&hir_id)
            .cloned()
            .unwrap_or_default();
        for r in regions {
            // Each region's release is relocated ahead of a frame-replacing tail
            // call that already closed this block, unless the call itself names
            // the region (docs/impl/region/relocate.md). Without an open
            // relocation point this is exactly the direct emission it wraps.
            self.with_tail_exit_hoist(r, |s| s.emit_decref_for_region(r, hir_id, result_reg));
        }
        self.emit_cell_content_drops(hir_id);
        // The ownership forest's co-owned-cycle cut: at the group's drop
        // site — the latest member `decref_point`, i.e. this node — free the whole member
        // set as one unit, replacing the members' individual decrefs (skipped above).
        // `owned_region_groups` is empty when no co-owned cycle is present, so this is
        // then inert.
        if let Some(members) = self.region_info.owned_region_groups.get(&hir_id).cloned() {
            self.emit_free_region_group(&members);
        }
        // The activation-owner cut: adopt each capture-back-edge SCC member into
        // the executing activation's owner node at the SCC's enclosing-scope site
        // (this node). The adopt transfers ownership only — the free is the
        // node's release at the activation's completion
        // (docs/impl/region/owner.md). Empty when no such SCC is present, so
        // then inert.
        if let Some(members) = self
            .region_info
            .activation_adopt_sites
            .get(&hir_id)
            .cloned()
        {
            self.emit_adopt_into_activation(&members);
        }
    }

    /// Emit the demise instructions for ONE region at `hir_id`'s `decref_point`,
    /// routed by the region's class (see [`Self::emit_decrefs_for`], which owns
    /// the per-node sequencing and the relocation wrapper).
    fn emit_decref_for_region(
        &mut self,
        r: crate::hir::region::Region,
        hir_id: HirId,
        result_reg: Option<Reg>,
    ) {
        {
            // A reassigned top-level binding's assign-value regions are released
            // by the store path (drop-on-overwrite for priors; the kept init-
            // region decref for the reaching value), so their ordinary slot-load
            // decref here is suppressed. See `analyze_regions_with`.
            if self.region_info.suppressed_decref_regions.contains(&r) {
                return;
            }
            // A co-owned-cycle member is freed
            // by the single `FreeRegionGroup` emitted at the group's drop site, not by
            // an individual decref — skip its own release. Empty when no co-owned
            // cycle is present, so then inert.
            if self.region_info.owned_group_members.contains(&r) {
                return;
            }
            // A builder-idiom merge child (a non-root region) carries no demise of
            // its own: its region is the merged root's, freed by the SINGLE
            // `DecrefRegion` at the root's (the outer aggregate's) `decref_point`,
            // which post-dominates the child's last use (docs/impl/region/merging.md,
            // gate 6 — a child never outlives its parent). After `static_slot`
            // canonicalization the child's own decref would name the same root slot;
            // emitting it would free the shared region at the child's earlier
            // `decref_point`, under the still-live parent (a use-after-free). Only
            // the root emits (`merged_root(root) == root`). With no merge,
            // `merged_root` is the identity and this never fires.
            if self.region_info.merged_root(r) != r {
                return;
            }
            if self.region_info.call_result_regions.contains(&r) {
                if let Some(&value_slot) = self.region_to_slot.get(&r) {
                    let slot = value_slot.index();
                    // Backstop — "a mutated slot is not a release route"
                    // (docs/impl/region/bindings.md). `slot` belongs to a
                    // reassigned binding, so by this region's `decref_point` the
                    // slot no longer holds the value whose region we mean to
                    // release — it holds whatever was last assigned. Loading it
                    // and decref'ing would free THAT live value
                    // (tests/elle/region-mutable-reassign-flow.lisp pins it).
                    // With no untainted route the release is skipped: an
                    // over-keep, never a mis-free. (When the suppression gate
                    // succeeds these regions are already in
                    // `suppressed_decref_regions` and never reach here; this only
                    // fires on the unsuppressed baseline.) Excludes capture-cell
                    // placeholders, whose own-region release below is correct.
                    if self.region_info.mutated_binding_value_regions.contains(&r)
                        && !self.region_info.cell_release_regions.contains(&r)
                    {
                        if crate::config::get().has_trace("rc") {
                            eprintln!(
                                "[trace:rc:emit] skip_value_route region={:?} mutated_slot={} span={}",
                                r, slot, self.current_span
                            );
                        }
                        return;
                    }
                    // A fn-local reassigned mutable binding owns this slot for its
                    // whole scope (`allocate_slot` never reuses a slot), so the
                    // slot holds the binding's OWN live value, not a dead value
                    // whose region is `r`. `region_to_slot[r]` names this slot only
                    // because `record_region_slot` recorded the binding's INIT (or
                    // a slot-resolved assign) region against it — for an
                    // immediate-valued counter (`(assign ii (%add ii 1))`) that
                    // region is spurious, and the analysis placed its `decref_point`
                    // inside the loop. Emitting the load+decref+nil-stamp zeroes the
                    // counter before its own increment reads it, so the loop never
                    // terminates. Skip the value route entirely. This never leaks a
                    // real accumulated value: a heap accumulator's producer
                    // (`(assign acc (f acc))`) is an ANF temp with its OWN let slot,
                    // and its scope-exit `DecrefValueRegion` routes through THAT
                    // slot — not the reassigned binding's — so it still fires
                    // (pinned by `tests/elle/region-tailcall-arg-transfer.lisp` and
                    // the `region-mutable-reassign-*` suite under `--wasm=full`).
                    // Excludes captured cells (env slots, released via
                    // `DecrefCellRegion`), which `reassigned_local_slots` never
                    // records. Pinned by
                    // `tests/elle/region-capture-cell-loop-uaf.lisp`.
                    if self.reassigned_local_slots.contains(&slot)
                        && !self.region_info.cell_release_regions.contains(&r)
                    {
                        if crate::config::get().has_trace("rc") {
                            eprintln!(
                                "[trace:rc:emit] skip_reassigned_slot_route region={:?} slot={} span={}",
                                r, slot, self.current_span
                            );
                        }
                        return;
                    }
                    // A captured env cell releases the BOX at its env index, never
                    // the inner value's caller-owned region.
                    if self.region_info.cell_release_regions.contains(&r) {
                        self.emit_cell_region_release(slot, hir_id);
                        return;
                    }
                    // Load the value from its slot and release by its
                    // runtime region. The slot still holds a dangling
                    // Value after this but is never read again
                    // (decref_point is the last use). A passthrough call
                    // whose result lives in a region this call did not
                    // allocate is handled at runtime: the release targets
                    // the value's actual runtime region, which the escape
                    // incref already balanced.
                    let val_reg = self.fresh_reg();
                    // Read the value from the space its slot was minted in
                    // ([`super::ValueSlot`]). An env-celled binding loads its
                    // cell RAW and lets `DecrefValueRegion` unwrap it:
                    // `result_region_of` sees through a capture cell to the
                    // content, which is the region this release names. That is
                    // the same route the top-level captured branch takes through
                    // its stack slot, and it keeps the cell unborrowed at the
                    // load — `LoadCapture` reads the content under a borrow the
                    // release path can still be holding.
                    match value_slot {
                        super::ValueSlot::Local(slot) => {
                            self.emit(LirInstr::LoadLocal { dst: val_reg, slot })
                        }
                        super::ValueSlot::Env(index) => self.emit(LirInstr::LoadCaptureRaw {
                            dst: val_reg,
                            index,
                        }),
                    }
                    // A transferred-returned-subtree consumer site: the release
                    // is REPLACED by `AdoptIntoActivation` — the adopt consumes
                    // the result region's whole count (the returned cycle's
                    // stuck back-edge reference included), and the activation
                    // owner node's completion release set-drops root + the
                    // producer-adopted interior members
                    // (docs/impl/region/owner.md). Same operand shape as the
                    // decref it replaces (one value consumed); the nil-stamp
                    // below still applies. Empty when no transfer subtree is present.
                    if self.region_info.transfer_adopt_regions.contains(&r) {
                        self.emit(LirInstr::AdoptIntoActivation { child: val_reg });
                        if crate::config::get().has_trace("rc") {
                            eprintln!(
                                "[trace:rc:emit] transfer_adopt region={:?} local_slot={} span={}",
                                r, slot, self.current_span
                            );
                        }
                    } else {
                        self.emit(LirInstr::DecrefValueRegion { src: val_reg });
                    }
                    // Clear the slot we just released. The decref_point is this
                    // value's last use, so the slot is never read again *for this
                    // value* — but the slot persists and is reused. A branch
                    // result is the UNION of its arms' regions (the region
                    // walk), so every arm region is released here by loading its
                    // own result slot. In a loop, an arm taken on a prior
                    // iteration left a still-live heap value in its slot (the
                    // value escaped — e.g. `put` into a table that outlives the
                    // loop); on an iteration that takes a DIFFERENT arm, this
                    // decref would reload that stale slot and over-free the
                    // escaped value (tests/elle/region-branch-result-loop-uaf.lisp
                    // pins it). Stamping
                    // nil makes the non-taken-arm release a no-op (region_of(nil)
                    // is None) while the taken arm rewrites its slot first, so the
                    // live value is still released exactly once.
                    //
                    // An ENV-celled value is not stamped: the write that would
                    // clear it is `StoreCapture`, whose funnel
                    // (`capture_store_with_rebind`) decrefs the content it
                    // displaces — releasing a second time the very reference
                    // this decref just took. The stamp guards a reused STACK
                    // slot; an env cell is one per binding per activation and is
                    // never reused, so there is nothing for it to guard.
                    if let Some(slot) = value_slot.local() {
                        if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                            self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                        }
                        // Record the route for the abandoned-frame walk
                        // (docs/impl/region/unwind.md). The slot IS the
                        // release: the instructions above release whatever it
                        // holds and stamp it nil, so an error exit finding a
                        // heap value there runs a release that did not run. Only
                        // this site records — a route the emitter declined above
                        // (a mutated binding, a reassigned slot, a cell release,
                        // a transfer adopt) has no entry, and neither does the
                        // env route, whose slot carries no nil stamp to say the
                        // release already ran.
                        if !self.current_func.frame_release_slots.contains(&slot) {
                            self.current_func.frame_release_slots.push(slot);
                        }
                    }
                    if crate::config::get().has_trace("rc") {
                        // The hir_id here is the `decref_point` HirId — where
                        // the regions analysis placed the release. Pair
                        // with [trace:rc:emit] emit_alloc on the same
                        // region id to spot a release placed at its own
                        // allocation's HirId.
                        eprintln!(
                            "[trace:rc:emit] emit_decref_value_region region={:?} local_slot={} hir_id={:?} static_slot={} span={}",
                            r,
                            slot,
                            hir_id,
                            self.static_slot(r),
                            self.current_span
                        );
                    }
                }
                // Unbound Call result. When this region is the node's OWN
                // result region — a DISCARDED call: ANF's propagating-tail
                // wrap keys the slot recording on the outer Let, so the
                // placeholder reaches its decref_point (this very call
                // node) with no slot — release by value off the freshly-
                // lowered result register (docs/impl/region/rules.md Rule 2,
                // "discarded result"). The guard excludes branch-union
                // regions whose decref_point lands here (their
                // alloc_region entry names a different node) — those keep
                // the slot path. Tail calls never reach here: return_sites
                // moves their decref_point to the Return node.
                //
                // Stack model: `DecrefValueRegion` CONSUMES its operand
                // (src/lir/emit/instr/ops.rs) and `result_reg`'s entry is the one
                // the parent expression still expects, so roundtrip
                // through the scratch slot — store (consumes), load a
                // fresh reg (pushed), release it (consumed), reload
                // `result_reg` (entry restored, value intact).
                else if let Some(src) =
                    result_reg.filter(|_| self.region_info.alloc_region.get(&hir_id) == Some(&r))
                {
                    let slot = self.scratch_slot();
                    self.emit(LirInstr::StoreLocal { slot, src });
                    let val_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    // A discarded transfer-consumer site (`(mk)` as a
                    // statement): the release is replaced by the activation
                    // adopt, exactly as on the slot path above.
                    if self.region_info.transfer_adopt_regions.contains(&r) {
                        self.emit(LirInstr::AdoptIntoActivation { child: val_reg });
                    } else {
                        self.emit(LirInstr::DecrefValueRegion { src: val_reg });
                    }
                    self.emit(LirInstr::LoadLocal { dst: src, slot });
                    if crate::config::get().has_trace("rc") {
                        eprintln!(
                            "[trace:rc:emit] emit_decref_value_region region={:?} discarded_result reg={:?} hir_id={:?} span={}",
                            r, src, hir_id, self.current_span
                        );
                    }
                }
                return;
            }
            // A release the relocation is about to REPLICATE into a branch's arms
            // has to name a value: only a value route nil-stamps the slot it read,
            // so only it counts once where two copies land on one path
            // (docs/impl/region/replicate.md). The id route below is the
            // default and keeps every release a single point already covers.
            if self.replicating_release {
                if let Some(slot) = self.value_release_slot(r) {
                    self.emit_slot_value_release(slot);
                    if crate::config::get().has_trace("rc") {
                        eprintln!(
                            "[trace:rc:emit] replicable_value_route region={:?} local_slot={} hir_id={:?} span={}",
                            r, slot, hir_id, self.current_span
                        );
                    }
                    return;
                }
            }
            let rid = self.static_slot(r);
            self.emit_decref_region(rid);
        }
    }

    /// The stack slot a value-routed release of `r` may load, or `None` where the
    /// region has no such route.
    ///
    /// `region_to_slot` is keyed on a region's ALLOCATION site, so the slot named
    /// here belongs to the binder whose init allocated `r` and holds that value
    /// from the binder to the release. Each refusal names a reason the slot is not
    /// what the release reads: a **mutated** binder repoints it (read off the
    /// region and, for a fn-local reassign, off the slot), an env cell's release
    /// names the BOX rather than the slot's value, and a transfer consumer's
    /// release is an `AdoptIntoActivation` rather than a decref. The final
    /// condition is the id route's own premise, asked here so the two routes rest
    /// on one fact: the lowerer must have stamped an allocation for this region,
    /// or the slot holds a value some other region owns.
    fn value_release_slot(&self, r: crate::hir::region::Region) -> Option<u16> {
        if self.region_info.mutated_binding_value_regions.contains(&r)
            || self.region_info.cell_release_regions.contains(&r)
            || self.region_info.transfer_adopt_regions.contains(&r)
        {
            return None;
        }
        let slot = self.region_to_slot.get(&r)?.local()?;
        if self.reassigned_local_slots.contains(&slot) {
            return None;
        }
        let stamped = self
            .region_to_table
            .get(&r)
            .is_some_and(|s| self.emitted_alloc_regions.contains(s));
        stamped.then_some(slot)
    }

    /// Emit `DecrefRegion` for a region's compiler-owned reference
    /// (the initial RC=1 that the compiler dropped at the region's
    /// `decref_point` HirId).
    ///
    /// Cross-region refs are decremented by cascade in `free_runtime_region_pages` at
    /// runtime, not by additional compiler-emitted `DecrefRegion`
    /// instructions. Compiler-emitted `IncrefRegion` (from
    /// `emit_increfs_for`) handles the incref side; cascade handles
    /// the decref side.
    pub(super) fn emit_decref_region(&mut self, region_id: StaticRegion) {
        // Suppress a phantom DecrefRegion: if the lowerer never stamped
        // an instruction with this region id via `emit_in_region`,
        // then the runtime has no entry for it and the DecrefRegion
        // would target the wrong region (or fail an assertion in
        // debug builds). A phantom arises where the region walk assigns
        // a region to a node whose lowering is transparent (DerefCell,
        // MakeCell). The walk gates its Begin/Letrec cells on the
        // lowerer's emit predicate; this guard covers every class the
        // walk does not gate.
        if !self.emitted_alloc_regions.contains(&region_id) {
            return;
        }
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] emit_decref_region hir_id={:?} region={} span={}",
                self.current_hir_id, region_id, self.current_span,
            );
        }
        // Record the slot for the abandoned-frame walk (docs/impl/region/unwind.md).
        // The slot-resolved
        // route's receipt is the activation map itself: the alloc mints the mapping
        // and this instruction TAKES it (`take_runtime_region_for_drop_slot`), so a
        // slot still mapped when the frame is abandoned is a release that did not
        // run. Only a slot this function actually emits for is recorded, so the map's
        // other entries — a caller's leftovers past a frame-replacing tail call —
        // stay out of the walk.
        if !self.current_func.frame_release_regions.contains(&region_id) {
            self.current_func.frame_release_regions.push(region_id);
        }
        self.emit(LirInstr::DecrefRegion { region_id });
    }
}
