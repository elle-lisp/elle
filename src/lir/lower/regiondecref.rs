// audited: 2026-11-14
//! Region releases: what the lowerer emits at each region's `decref_point`, routed by the region's class.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/rules.md
//! docs/impl/region/unwind.md
//!
//! Split by what a release names, all methods hanging off the shared
//! `impl<'a> Lowerer<'a>`:
//!   - this file: the per-node sequencing, the per-region routing, and the
//!     slot-resolved `DecrefRegion`
//!   - `classify`: the route a release reads its value by, and its refusals
//!   - `routes`: the value, env-cell, 1-slot-container and co-owned-group routes
//!   - `arms`:   the per-arm compensation a branch owes

use super::*;

mod arms;
mod classify;
mod routes;

pub(in crate::lir::lower) use classify::ReleaseRoute;

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
        // The ownership forest's co-owned-cycle cut: at the group's drop site — the
        // latest member `decref_point`, that is this node — free the whole member set
        // as one unit, replacing the members' individual decrefs (skipped above).
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
        let route = self.release_route(r);
        if route == ReleaseRoute::Elsewhere {
            return;
        }
        if self.region_info.call_result_regions.contains(&r) {
            match route {
                ReleaseRoute::Elsewhere => {}
                ReleaseRoute::Refused(why) => {
                    if crate::config::get().has_trace("rc") {
                        eprintln!(
                            "[trace:rc:emit] skip_value_route region={:?} refusal={:?} span={}",
                            r, why, self.current_span
                        );
                    }
                }
                ReleaseRoute::Cell(index) => self.emit_cell_region_release(index, hir_id),
                ReleaseRoute::Adopt(slot) | ReleaseRoute::Value(slot) => {
                    // Load the value from its slot and release it by its runtime
                    // region. The `decref_point` is the value's last use, so the
                    // slot is never read again for this value. A passthrough
                    // call whose result lives in a region this call did not
                    // allocate is handled at runtime: the release targets the
                    // value's actual runtime region, which the escape incref
                    // already balanced.
                    //
                    // The slot persists and is reused, which is what the nil
                    // stamp is for. A branch result is the UNION of its arms'
                    // regions, so every arm region is released here by loading
                    // its own result slot. In a loop, an arm taken on a prior
                    // iteration left a still-live heap value in its slot (the
                    // value escaped — for example `put` into a table that
                    // outlives the loop). On an iteration that takes a
                    // DIFFERENT arm, this release would reload that stale slot
                    // and over-free the escaped value
                    // (tests/impl/region-branch-result-loop-uaf.lisp pins it).
                    // The stamp makes the non-taken arm's release a no-op, while
                    // the taken arm rewrites its slot first.
                    let adopt = matches!(route, ReleaseRoute::Adopt(_));
                    self.emit_routed_release(slot, adopt);
                    // Record the route for the abandoned-frame walk
                    // (docs/impl/region/unwind.md). The slot IS the release: an
                    // error exit finding a heap value there runs a release that
                    // did not run. The env route has no entry, because its slot
                    // carries no nil stamp to say the release already ran.
                    if let Some(local) = slot.local() {
                        self.record_value_route(local);
                    }
                    if crate::config::get().has_trace("rc") {
                        // The hir_id here is the `decref_point` HirId — where
                        // the regions analysis placed the release. Pair
                        // with [trace:rc:emit] emit_alloc on the same
                        // region id to spot a release placed at its own
                        // allocation's HirId.
                        eprintln!(
                            "[trace:rc:emit] emit_decref_value_region region={:?} slot={:?} adopt={} hir_id={:?} static_slot={} span={}",
                            r,
                            slot,
                            adopt,
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
                ReleaseRoute::Unslotted => {
                    let Some(src) = result_reg
                        .filter(|_| self.region_info.alloc_region.get(&hir_id) == Some(&r))
                    else {
                        return;
                    };
                    let slot = self.scratch_slot();
                    self.emit(InstrRef::StoreLocal { slot, src });
                    let val_reg = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal { dst: val_reg, slot });
                    // A discarded transfer-consumer site (`(mk)` as a
                    // statement): the release is replaced by the activation
                    // adopt, exactly as on the slot path above.
                    if self.region_info.transfer_adopt_regions.contains(&r) {
                        self.emit(InstrRef::AdoptIntoActivation { child: val_reg });
                    } else {
                        self.emit(InstrRef::DecrefValueRegion { src: val_reg });
                    }
                    self.emit(InstrRef::LoadLocal { dst: src, slot });
                    if crate::config::get().has_trace("rc") {
                        eprintln!(
                            "[trace:rc:emit] emit_decref_value_region region={:?} discarded_result reg={:?} hir_id={:?} span={}",
                            r, src, hir_id, self.current_span
                        );
                    }
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

    /// The stack slot a value-routed release of `r` may load in place of its
    /// release by id, or `None` where the region has no such route.
    ///
    /// `region_to_slot` is keyed on a region's ALLOCATION site, so the slot named
    /// here belongs to the binder whose init allocated `r` and holds that value
    /// from the binder to the release. The route is the one [`ReleaseRoute`] reads,
    /// and only a plain `Value` route in a stack slot stamps it. The final
    /// condition is the id route's own premise, asked here so the two routes rest
    /// on one fact: the lowerer must have stamped an allocation for this region,
    /// or the slot holds a value some other region owns.
    fn value_release_slot(&self, r: crate::hir::region::Region) -> Option<u16> {
        let ReleaseRoute::Value(super::ValueSlot::Local(slot)) = self.release_route(r) else {
            return None;
        };
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
        // The slot-resolved route's receipt is the activation map itself: the alloc
        // mints the mapping and this instruction TAKES it
        // (`take_runtime_region_for_drop_slot`), so a slot still mapped when the
        // frame is abandoned is a release that did not run. Only a slot this function
        // actually emits for is recorded, so the map's other entries — a caller's
        // leftovers past a frame-replacing tail call — stay out of the walk.
        let head = self.head();
        if !head.frame_release_regions.contains(&region_id) {
            head.frame_release_regions.push(region_id);
        }
        self.emit(InstrRef::DecrefRegion { region_id });
    }
}
