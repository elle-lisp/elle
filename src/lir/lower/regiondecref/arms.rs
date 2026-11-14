// audited: 2026-11-14
//! Per-arm compensation: the release a branch arm owes for a region whose `decref_point` sits in a sibling arm.
//!
//! docs/impl/region/compensate.md

use super::*;
use crate::lir::lower::ValueSlot;

impl<'a> Lowerer<'a> {
    /// Emit the per-arm release for any region whose `branch_arm_decrefs` entry
    /// names this node (a region's last use within a sibling arm of an `If`/`Match`
    /// whose `decref_point` is in a DIFFERENT arm). Called AFTER the node's own
    /// `emit_decrefs_for`, so it fires after the arm's use of the value. Restricted
    /// by the analysis (`region::infer::compensate`) to single-holder `call_result`
    /// regions and to env cells, so only the slot routes and the box route apply.
    /// Mutually exclusive arms ⇒ exactly one of these (or the `decref_point`) fires
    /// per path.
    pub(in crate::lir::lower) fn emit_arm_decrefs(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_arm_decrefs.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            let route = self.release_route(r);
            self.emit_arm_release(r, route, hir_id, "arm_decref");
        }
    }

    /// Emit the per-path compensating release for any region whose
    /// `branch_compensation` entry names this node (a branch arm body). The
    /// region's true `decref_point` lives in a SIBLING arm, so it leaks on this
    /// arm — this head-of-arm release frees it once on this path, before the arm's
    /// own body (hence before any tail call the arm makes). Called at the top of
    /// `lower_expr`, so it lands inside the arm's basic block. Mirrors
    /// `emit_decrefs_for`'s per-region routing — call-result regions release by
    /// their slot route, all others by region id — for the same classes, minus the
    /// discarded-result/group paths the analysis (`region::infer::compensate`)
    /// excludes. See that module for why exactly one of the two releases fires per
    /// path.
    pub(in crate::lir::lower) fn emit_branch_compensation(&mut self, hir_id: HirId) {
        let regions = match self.region_info.branch_compensation.get(&hir_id) {
            Some(rs) => rs.clone(),
            None => return,
        };
        for r in regions {
            let route = self.release_route(r);
            if route == ReleaseRoute::Elsewhere {
                continue;
            }
            if self.region_info.call_result_regions.contains(&r) {
                self.emit_arm_release(r, route, hir_id, "branch_compensation");
            } else {
                let rid = self.static_slot(r);
                self.emit_decref_region(rid);
            }
        }
    }

    /// The release an arm owes at `hir_id` for `r`, whose `decref_point` takes
    /// `route`.
    fn emit_arm_release(
        &mut self,
        r: crate::hir::region::Region,
        route: ReleaseRoute,
        hir_id: HirId,
        what: &str,
    ) {
        match route {
            // An env cell's box, whether this arm read the cell's binding or
            // named it nowhere. Routed to the box rather than through the
            // holder's slot, which keeps the holder's reassignment and its
            // capturers out of the question.
            ReleaseRoute::Cell(index) => self.emit_cell_region_release(index, hir_id),
            // Stack-only: the analysis restricts this compensation to
            // single-holder `call_result` regions, and an env-indexed value slot
            // is not one, so skipping it leaves the region on its own
            // `decref_point`. The value was allocated before the branch, so the
            // slot is live entering this arm, and the nil stamp keeps a later
            // reuse of the slot from being mistaken for this freed value.
            route @ (ReleaseRoute::Value(ValueSlot::Local(slot))
            | ReleaseRoute::Adopt(ValueSlot::Local(slot))) => {
                let adopt = matches!(route, ReleaseRoute::Adopt(_));
                self.emit_routed_release(ValueSlot::Local(slot), adopt);
                if crate::config::get().has_trace("rc") {
                    eprintln!(
                        "[trace:rc:emit] {what} region={:?} local_slot={} adopt={} node={:?} span={}",
                        r, slot, adopt, hir_id, self.current_span
                    );
                }
            }
            // Another mechanism releases the region, or no route reads its
            // value here: a release owned elsewhere must never be doubled, and
            // a refused slot holds a later value by now.
            ReleaseRoute::Elsewhere
            | ReleaseRoute::Unslotted
            | ReleaseRoute::Refused(_)
            | ReleaseRoute::Value(ValueSlot::Env(_))
            | ReleaseRoute::Adopt(ValueSlot::Env(_)) => {}
        }
    }
}
