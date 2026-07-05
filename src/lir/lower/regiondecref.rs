use super::*;

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
    ///   lowered result (docs/impl/region-rules.md Rule 2, "discarded result").
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
        // `with_region_info` (was a linear scan of all regions per node).
        //
        // No tail-region suppression: ownership transfer to the caller is
        // now carried by `IncrefValueRegion` (emitted at each `Return`),
        // not by withholding the callee's `DecrefRegion`. A freshly-
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
            // A reassigned top-level binding's assign-value regions are released
            // by the store path (drop-on-overwrite for priors; the kept init-
            // region decref for the reaching value), so their ordinary slot-load
            // decref here is suppressed. See `analyze_regions_with`.
            if self.region_info.suppressed_decref_regions.contains(&r) {
                continue;
            }
            // A self-recursive `def` binding's cell-free closure region: its
            // compiler-emitted `DecrefRegion` is stranded — the runtime adopt at the
            // `(loop …)` tail call is the sole release. Emitting it here would free the
            // region before the tail call re-enters the closure living in it (a
            // use-after-free). Empty unless a self-recursive `def` was lowered.
            if self.suppressed_self_regions.contains(&r) {
                continue;
            }
            // A co-owned-cycle member is freed
            // by the single `FreeRegionGroup` emitted below at the group's drop site, not
            // by an individual decref — skip its own release. Empty without the flag, so
            // this is inert on the baseline path.
            if self.region_info.owned_group_members.contains(&r) {
                continue;
            }
            // A builder-idiom merge child (a non-root region) carries no demise of
            // its own: its region is the merged root's, freed by the SINGLE
            // `DecrefRegion` at the root's (the outer aggregate's) `decref_point`,
            // which post-dominates the child's last use (region-model.md § Merging
            // gate 6 — a child never outlives its parent). After `static_slot`
            // canonicalization the child's own decref would name the same root slot;
            // emitting it would free the shared region at the child's earlier
            // `decref_point`, under the still-live parent (a use-after-free). Only
            // the root emits (`merged_root(root) == root`). With no merge,
            // `merged_root` is the identity and this never fires.
            if self.region_info.merged_root(r) != r {
                continue;
            }
            if self.region_info.call_result_regions.contains(&r) {
                if let Some(&slot) = self.region_to_slot.get(&r) {
                    // Backstop — "a mutated slot is not a release route"
                    // (docs/impl/region-bindings.md). `slot` belongs to a
                    // reassigned binding, so by this region's `decref_point` the
                    // slot no longer holds the value whose region we mean to
                    // release — it holds whatever was last assigned. Loading it
                    // and decref'ing would free THAT live value (the no-alias
                    // corruption UAF: region-mutable-reassign-flow facet 3 — `rd`
                    // reads `rc`'s reassigned value because `rc`'s init-list
                    // decref, routed through `rc`'s slot, frees a live region).
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
                        continue;
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
                    if self.region_info.cell_release_regions.contains(&r) {
                        // Captured env cell (an `@x` lbox / captured-local cell):
                        // `slot` is the upvalue/env index. Load the CELL itself
                        // (raw, no deref) and free the CELL's OWN region via
                        // `DecrefCellRegion` (region_of) — never unwrap to the
                        // inner value's caller-owned region. The closure-capture
                        // incref keeps the cell alive past this release until the
                        // capturing closure's region cascade-frees it.
                        self.emit(LirInstr::LoadCaptureRaw {
                            dst: val_reg,
                            index: slot,
                        });
                        self.emit(LirInstr::DecrefCellRegion { src: val_reg });
                        if crate::config::get().has_trace("rc") {
                            eprintln!(
                                "[trace:rc:emit] emit_decref_cell_region hir_id={:?} upvalue_slot={} span={}",
                                hir_id, slot, self.current_span
                            );
                        }
                        continue;
                    }
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    // A transferred-returned-subtree consumer site: the release
                    // is REPLACED by `AdoptIntoActivation` — the adopt consumes
                    // the result region's whole count (the returned cycle's
                    // stuck back-edge reference included), and the activation
                    // owner node's completion release set-drops root + the
                    // producer-adopted interior members
                    // (docs/impl/region-model.md § "Owner nodes" — "The
                    // transferred returned subtree"). Same operand shape as the
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
                    // result is the UNION of its arms' regions (regions.rs
                    // `walk`), so every arm region is released here by loading its
                    // own result slot. In a loop, an arm taken on a prior
                    // iteration left a still-live heap value in its slot (the
                    // value escaped — e.g. `put` into a table that outlives the
                    // loop); on an iteration that takes a DIFFERENT arm, this
                    // decref would reload that stale slot and over-free the
                    // escaped value (the branch-result-loop UAF — see
                    // tests/elle/region-branch-result-loop-uaf.lisp). Stamping
                    // nil makes the non-taken-arm release a no-op (region_of(nil)
                    // is None) while the taken arm rewrites its slot first, so the
                    // live value is still released exactly once.
                    if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                        self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                    }
                    if crate::config::get().has_trace("rc") {
                        // The hir_id here is the `decref_point` HirId — where
                        // the regions analysis placed the release. Pair
                        // with [trace:rc:emit] emit_alloc on the same
                        // region id to spot the alloc-then-release-at-
                        // same-HirId pattern (the bug class fixed by
                        // propagating parent_consumes through Or/And/
                        // If branches/Let body/Begin tail in
                        // src/hir/liveness.rs).
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
                // lowered result register (docs/impl/region-rules.md Rule 2,
                // "discarded result"). The guard excludes branch-union
                // regions whose decref_point lands here (their
                // alloc_region entry names a different node) — those keep
                // the slot path. Tail calls never reach here: return_sites
                // moves their decref_point to the Return node.
                //
                // Stack model: `DecrefValueRegion` CONSUMES its operand
                // (emit/instr/ops.rs) and `result_reg`'s entry is the one
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
                continue;
            }
            let rid = self.static_slot(r);
            self.emit_decref_region(rid);
        }
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
        // (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge
        // SCC"). Empty when no such SCC is present, so then inert.
        if let Some(members) = self
            .region_info
            .activation_adopt_sites
            .get(&hir_id)
            .cloned()
        {
            self.emit_adopt_into_activation(&members);
        }
    }
    /// Emit `FreeRegionGroup` for a co-owned region group: load every member's value
    /// from its binding slot to drive
    /// the value-resolved free, then emit the one instruction that frees the whole set
    /// as a unit. A member with no value-resolved home (no `region_to_slot` entry) leaves
    /// the group unfreed — the always-legal fallback (the members stay independently RC'd,
    /// as without the flag), mirroring `emit_adopt_region`'s missing-slot skip.
    fn emit_free_region_group(&mut self, members: &[crate::hir::region::Region]) {
        let mut regs = Vec::with_capacity(members.len());
        for &m in members {
            let Some(&slot) = self.region_to_slot.get(&m) else {
                return;
            };
            let reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: reg, slot });
            regs.push(reg);
        }
        if regs.is_empty() {
            return;
        }
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] free_region_group members={:?}",
                members.iter().map(|r| r.0).collect::<Vec<_>>()
            );
        }
        self.emit(LirInstr::FreeRegionGroup { members: regs });
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
        // Suppress phantom DecrefRegion: if the lowerer never stamped
        // an instruction with this region id via `emit_in_region`,
        // then the runtime has no entry for it and the DecrefRegion
        // would target the wrong region (or fail an assertion in
        // debug builds). Phantom regions can arise from regions-walk
        // assignments to nodes whose lowering layer is transparent
        // (DerefCell, MakeCell) or from analysis gaps that the
        // ongoing audit hasn't yet closed. Begin/Letrec phantoms (the
        // if-phi-merge case) were fixed at the analysis layer in
        // src/hir/regions.rs by gating alloc_here on the lowerer's
        // emit predicate; other classes (Match without captured pattern
        // bindings, etc.) remain to be audited — the guard stays as
        // defense in depth until that audit is complete.
        if !self.emitted_alloc_regions.contains(&region_id) {
            return;
        }
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] emit_decref_region hir_id={:?} region={} span={}",
                self.current_hir_id, region_id, self.current_span,
            );
        }
        self.emit(LirInstr::DecrefRegion { region_id });
    }

    /// Emit the per-arm release for any region whose `branch_arm_decrefs` entry
    /// names this node (a region's last use within a sibling arm of an `If`/`Match`
    /// whose `decref_point` is in a DIFFERENT arm). Called AFTER the node's own
    /// `emit_decrefs_for`, so it fires after the arm's use of the value. Restricted
    /// by the analysis (`regions::compensate`) to single-holder `call_result`
    /// regions, so only the value-route applies. Mutually exclusive arms ⇒ exactly
    /// one of these (or the `decref_point`) fires per path.
    pub(super) fn emit_arm_decrefs(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_arm_decrefs.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            // Defensive: a release owned by another mechanism must never be doubled.
            if self.region_info.suppressed_decref_regions.contains(&r)
                || self.region_info.owned_group_members.contains(&r)
                || self.region_info.cell_release_regions.contains(&r)
                || self.region_info.mutated_binding_value_regions.contains(&r)
                || self.region_info.merged_root(r) != r
            {
                continue;
            }
            if let Some(&slot) = self.region_to_slot.get(&r) {
                let val_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                self.emit(LirInstr::DecrefValueRegion { src: val_reg });
                if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                    self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                }
                if crate::config::get().has_trace("rc") {
                    eprintln!(
                        "[trace:rc:emit] arm_decref region={:?} local_slot={} arm_node={:?} span={}",
                        r, slot, hir_id, self.current_span
                    );
                }
            }
        }
    }

    /// Emit the per-path compensating release for any region whose
    /// `branch_compensation` entry names this node (a branch arm body). The
    /// region's true `decref_point` lives in a SIBLING arm, so it leaks on this
    /// arm — this head-of-arm release frees it once on this path, before the arm's
    /// own body (hence before any tail call the arm makes). Called at the top of
    /// `lower_expr`, so it lands inside the arm's basic block. Mirrors
    /// `emit_decrefs_for`'s per-region routing — call-result regions release by
    /// value off the holder slot (then nil-stamp it), all others by region id —
    /// for the same classes, minus the discarded-result/group paths the analysis
    /// (`regions::compensate`) excludes. See that module for why exactly one of
    /// the two releases fires per path.
    pub(super) fn emit_branch_compensation(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_compensation.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            // Defensive: a release owned by another mechanism must never be
            // doubled here (the analysis already excludes these).
            if self.region_info.suppressed_decref_regions.contains(&r)
                || self.region_info.owned_group_members.contains(&r)
                || self.region_info.cell_release_regions.contains(&r)
                || self.region_info.merged_root(r) != r
            {
                continue;
            }
            if self.region_info.call_result_regions.contains(&r) {
                // A mutated-slot binding is not a release route (its slot holds a
                // later value by now) — skip, as `emit_decrefs_for` does.
                if self.region_info.mutated_binding_value_regions.contains(&r) {
                    continue;
                }
                // Value-route: the value was allocated before the branch, so the
                // holder slot is live entering this (dead-on-this-path) arm. Load
                // it, release its runtime region, then nil-stamp so a later reuse
                // of the slot is not mistaken for this freed value.
                if let Some(&slot) = self.region_to_slot.get(&r) {
                    let val_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    self.emit(LirInstr::DecrefValueRegion { src: val_reg });
                    if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                        self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                    }
                    if crate::config::get().has_trace("rc") {
                        eprintln!(
                            "[trace:rc:emit] branch_compensation region={:?} local_slot={} arm={:?} span={}",
                            r, slot, hir_id, self.current_span
                        );
                    }
                }
                continue;
            }
            let rid = self.static_slot(r);
            self.emit_decref_region(rid);
        }
    }
}
