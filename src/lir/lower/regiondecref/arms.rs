// audited: 2026-09-23
//! Per-arm compensation: the release a branch arm owes for a region whose `decref_point` sits in a sibling arm.
//!
//! docs/impl/region/compensate.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Emit the per-arm release for any region whose `branch_arm_decrefs` entry
    /// names this node (a region's last use within a sibling arm of an `If`/`Match`
    /// whose `decref_point` is in a DIFFERENT arm). Called AFTER the node's own
    /// `emit_decrefs_for`, so it fires after the arm's use of the value. Restricted
    /// by the analysis (`region::infer::compensate`) to single-holder `call_result`
    /// regions and to env cells, so only the value route and the box route apply.
    /// Mutually exclusive arms ⇒ exactly one of these (or the `decref_point`) fires
    /// per path.
    pub(in crate::lir::lower) fn emit_arm_decrefs(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_arm_decrefs.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            // Defensive: a release owned by another mechanism must never be doubled.
            if self.region_info.suppressed_decref_regions.contains(&r)
                || self.region_info.owned_group_members.contains(&r)
                || self.region_info.merged_root(r) != r
            {
                continue;
            }
            // An env cell: this arm READ the cell's binding, so its box release
            // lands here — after that read — rather than at the arm head. Routed to
            // the box rather than through the holder's slot, which is what keeps the
            // holder's reassignment and its capturers out of the question.
            if self.region_info.cell_release_regions.contains(&r) {
                if let Some(&value_slot) = self.region_to_slot.get(&r) {
                    self.emit_cell_region_release(value_slot.index(), hir_id);
                }
                continue;
            }
            if self.region_info.mutated_binding_value_regions.contains(&r) {
                continue;
            }
            // Stack-only: the analysis restricts this compensation to
            // single-holder `call_result` regions, and an env-indexed value slot is
            // not one — skipping leaves the region on its own `decref_point`.
            if let Some(slot) = self.region_to_slot.get(&r).and_then(|s| s.local()) {
                self.emit_slot_value_release(slot);
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
    /// (`region::infer::compensate`) excludes. See that module for why exactly one of
    /// the two releases fires per path.
    pub(in crate::lir::lower) fn emit_branch_compensation(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_compensation.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            // Defensive: a release owned by another mechanism must never be
            // doubled here (the analysis already excludes these).
            if self.region_info.suppressed_decref_regions.contains(&r)
                || self.region_info.owned_group_members.contains(&r)
                || self.region_info.merged_root(r) != r
            {
                continue;
            }
            // An env cell: this arm names the cell's binding nowhere, so the box's
            // own `decref_point` release sits in a mutually-exclusive sibling arm
            // and this head copy is the only one this path runs. Routed to the box
            // rather than through the holder's slot, which is what keeps the
            // holder's reassignment and its capturers out of the question.
            if self.region_info.cell_release_regions.contains(&r) {
                if let Some(&value_slot) = self.region_to_slot.get(&r) {
                    self.emit_cell_region_release(value_slot.index(), hir_id);
                }
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
                // of the slot is not mistaken for this freed value. Stack-only,
                // as `emit_arm_decrefs` is.
                if let Some(slot) = self.region_to_slot.get(&r).and_then(|s| s.local()) {
                    self.emit_slot_value_release(slot);
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
