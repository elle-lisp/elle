// audited: 2026-09-14
//! What a node emits for the references its own stores take.
//!
//! The cross-region incref, the ownership adopt that stands in for one, and the
//! region frees a tail-call site defers to itself.
//! docs/impl/region/mechanism.md
//! docs/impl/region/ownership.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Emit `AdoptRegion(parent, child)` for an interior owned-subtree edge: load
    /// both values from their binding slots and link the child's runtime region
    /// into the parent's Owned subtree (docs/impl/region/ownership.md § "Adoption and
    /// subtree drop"). A missing slot for an endpoint (no value-resolved home)
    /// skips the adopt — the regions then stay independently RC'd, the always-legal
    /// fallback (the frozen-RC contract makes the skip correctness-neutral).
    fn emit_adopt_region(
        &mut self,
        child: crate::hir::region::Region,
        parent: crate::hir::region::Region,
    ) {
        // Stack-only, like the other adopt emitters: an env-celled endpoint
        // skips the adopt and leaves both regions independently reference
        // counted — the always-legal fallback a missing slot already takes.
        let (Some(pslot), Some(cslot)) = (
            self.region_to_slot.get(&parent).and_then(|s| s.local()),
            self.region_to_slot.get(&child).and_then(|s| s.local()),
        ) else {
            return;
        };
        let preg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: preg,
            slot: pslot,
        });
        let creg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: creg,
            slot: cslot,
        });
        self.emit(LirInstr::AdoptRegion {
            parent: preg,
            child: creg,
        });
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] adopt_region child={} parent={}",
                child.0, parent.0
            );
        }
    }

    /// Emit `AdoptIntoActivation` for each member of a capture-back-edge SCC:
    /// load the member's value from its binding slot and adopt its runtime
    /// region into the executing activation's owner node
    /// (docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge
    /// SCC"). The adopt transfers ownership only; the free is the node's release
    /// at the activation's completion. A member with no slot is skipped — its
    /// region stays `Counted` and, its decref being suppressed, over-kept to
    /// teardown: a bounded fallback, never a double-free. Slots are deduped so
    /// two member regions resolving to one slot (a branch-dependent union)
    /// adopt once, keeping the runtime's one-adoption assert unreachable (the
    /// admission's pairwise-distinct-holder gate makes both cases unreachable
    /// in practice; this is the emit-side belt).
    pub(in crate::lir::lower) fn emit_adopt_into_activation(
        &mut self,
        members: &[crate::hir::region::Region],
    ) {
        let mut seen: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
        for &m in members {
            // Stack-only (see `emit_adopt_region`): an env-celled member is
            // skipped, its region staying `Counted` and over-kept to teardown.
            let Some(slot) = self.region_to_slot.get(&m).and_then(|s| s.local()) else {
                continue;
            };
            if !seen.insert(slot) {
                continue;
            }
            let reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: reg, slot });
            self.emit(LirInstr::AdoptIntoActivation { child: reg });
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc:emit] adopt_into_activation member={} local_slot={}",
                    m.0, slot
                );
            }
        }
    }

    /// Emit IncrefRegion for any cross-region references at this HIR node.
    pub(in crate::lir::lower) fn emit_increfs_for(&mut self, hir_id: HirId) {
        // Ownership forest: an interior edge of an externally-unique Owned subtree
        // becomes an `AdoptRegion` (parent adopts the child's region; no RC),
        // emitted here in place of the edge's `IncrefRegion`. `owned_adopt_edges`
        // is empty for a shape that stays Shared, so this is then inert
        // (docs/impl/region/ownership.md § "Adoption and subtree drop").
        let adopt_edges = self
            .region_info
            .owned_adopt_edges
            .get(&hir_id)
            .cloned()
            .unwrap_or_default();
        for &(child, parent) in &adopt_edges {
            self.emit_adopt_region(child, parent);
        }
        // O(1) lookup into the site-indexed map built in
        // `with_region_info` (was a linear scan of every cross-region ref
        // per node — O(n²) over a large compilation unit). Each entry is an
        // edge `(source, target)`: `source` is the region the incref names,
        // `target` rides along so a post-merge intra-region self-edge can be
        // detected (`is_merge_self_edge`) and dropped (transform 2, below).
        let refs: Vec<_> = match self.increfs_by_site.get(&hir_id) {
            Some(edges) => edges.clone(),
            None => return,
        };
        // A 1-slot container OWNS the count on its content: the cell holds
        // exactly one reference, taken by `lower_define`'s incref-on-store (or
        // donated outright from the producer), and released by drop-on-overwrite
        // for each displaced prior. The `cell ⊇ content` edge recorded at the
        // store names that same reference, so counting it here is the second
        // count of one holding — and the edge's own balancing decref is the
        // TARGET's free-time cascade, which fires once per scope while a loop
        // stores every iteration. Pinned by
        // `reassign_toplevel_prior_release_is_bounded` and the fn-local leak
        // faces in `region-fn-local-cell-drop-leak.lisp`.
        if self.region_info.drop_on_overwrite_sites.contains(&hir_id) {
            return;
        }
        let hard_site = self.region_info.hard_edge_sites.contains(&hir_id);
        for (src, dst) in refs {
            // An interior owned-subtree edge's reference count is replaced by the
            // `AdoptRegion` emitted above (the subtree frees as a unit) — skip its
            // incref. Inert on the baseline path (`adopt_edges` empty).
            if adopt_edges.contains(&(src, dst)) {
                continue;
            }
            // A call-result region is a marker, not a prediction: its static
            // slot is never populated at runtime (only alloc opcodes record
            // region mints in the activation map), so a slot-based
            // `IncrefRegion` against it resolves to nothing and the edge's
            // balancing decref — the store target's free-time cascade — then
            // steals a live reference (the call-result-arg clique UAF,
            // tests/elle/region-native-clique-callresult-uaf.lisp). At a HARD
            // edge site (a declared native uncounted-store effect —
            // docs/impl/region/effects.md "Hard edges") incref by VALUE instead, the
            // exact mirror of `emit_decrefs_for`'s call-result branch: load
            // the value from its binding slot and retain the runtime region
            // it actually lives in. Opaque user-fn sites keep the slot path
            // (the no-op — a real incref there never balances
            // when the callee stores through the runtime funnel;
            // region-userfn-clique-callresult-noleak.lisp). Cell phantom
            // placeholders keep the slot path too (their `region_to_slot`
            // entry is an upvalue index, not a local slot).
            if hard_site
                && self.region_info.call_result_regions.contains(&src)
                && !self.region_info.cell_release_regions.contains(&src)
            {
                if let Some(slot) = self.region_to_slot.get(&src).and_then(|s| s.local()) {
                    let val_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
                    self.emit(LirInstr::IncrefValueRegion { src: val_reg });
                    // `IncrefValueRegion` peeks (Return-position contract: the
                    // value must stay on top for the caller); mid-stream that
                    // leaves an unconsumed entry that skews the emitter's
                    // stack model. Store the value back to its own slot — a
                    // semantic no-op whose emission consumes the entry.
                    self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                }
                // No slot: nothing to load — the same net no-op as the
                // unpopulated-slot `IncrefRegion` this replaces.
                continue;
            }
            // Self-edge elimination (transform 2; docs/impl/region/mechanism.md
            // § "Self-edge elimination"). A builder-idiom merge collapses this
            // `src → dst` store edge's endpoints onto one region (they share a
            // `merged_root`), making it an intra-region self-edge. The free-time
            // cascade skips a region's references into itself
            // (`regionpool/introspect.rs`, `rid != own_id`), so its
            // `IncrefRegion(root)` would have no balancing decref — keeping it leaks
            // the merged region. Drop it. This is the emission counterpart of
            // `static_slot`'s canonicalization and `emit_decrefs_for`'s child-decref
            // suppression: the three move together, taking the edge from
            // cross-region (incref + cascade decref) to intra-region (no RC).
            if self.region_info.is_merge_self_edge(src, dst) {
                super::rcstats::record_self_edge_eliminated();
                if crate::config::get().has_trace("rc") {
                    eprintln!(
                        "[trace:rc:emit] merge_self_edge_eliminated source={} target={} hir_id={:?} span={}",
                        src.0, dst.0, hir_id, self.current_span
                    );
                }
                continue;
            }
            let region_id = self.static_slot(src);
            self.emit(LirInstr::IncrefRegion { region_id });
        }
    }
    /// Emit pending `DecrefRegion` instructions. Called at tail-call
    /// sites where region cleanup is deferred. Deduplicates to avoid
    /// double-decrementing regions shared between nested scopes.
    pub(in crate::lir::lower) fn emit_pending_free_regions(&mut self) {
        let pending: Vec<StaticRegion> = self.pending_free_regions.clone();
        let mut seen = std::collections::HashSet::new();
        for region_id in pending {
            if seen.insert(region_id) {
                self.emit_decref_region(region_id);
            }
        }
    }
}
