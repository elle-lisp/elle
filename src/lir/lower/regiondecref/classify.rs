// audited: 2026-11-21
//! The route a region's release reads its value by, decided once for every site that releases the region.
//!
//! src/lir/lower/regions.md
//! docs/impl/region/bindings.md
//! docs/impl/region/mechanism.md

use super::*;
use crate::hir::region::Region;
use crate::lir::lower::ValueSlot;

/// How a release of a region reads its value. The region's `decref_point`, a
/// branch arm's compensation, a relocation's replica and a call's move each
/// read it, so none of them can release by a route the others refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lir::lower) enum ReleaseRoute {
    /// Another mechanism releases the region: the store path, its co-owned
    /// group's free, or its merged root.
    Elsewhere,
    /// No slot names the value.
    Unslotted,
    /// The slot no longer holds the value, so nothing releases the region
    /// here: an over-keep, never a mis-free.
    Refused(Refusal),
    /// The env cell's box, at its env index: `DecrefCellRegion`, never the
    /// inner value's caller-owned region.
    Cell(u16),
    /// The slot's value, adopted into the activation.
    Adopt(ValueSlot),
    /// The slot's value, released by `DecrefValueRegion`.
    Value(ValueSlot),
}

/// Why the slot no longer holds the value a release means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lir::lower) enum Refusal {
    /// The binding is a top-level reassigned one.
    Mutated,
    /// The slot is a fn-local reassigned binding's.
    Reassigned,
}

impl<'a> Lowerer<'a> {
    /// The route `r`'s release reads its value by.
    pub(in crate::lir::lower) fn release_route(&self, r: Region) -> ReleaseRoute {
        let info = &self.region_info;
        // A reassigned top-level binding's assign-value regions are released
        // by the store path (drop-on-overwrite for priors; the kept init-region
        // decref for the reaching value). See `analyze_regions_with`.
        //
        // A co-owned-cycle member is freed by the single `FreeRegionGroup`
        // emitted at the group's drop site.
        //
        // A builder-idiom merge child (a non-root region) carries no demise of
        // its own: its region is the merged root's, freed by the SINGLE
        // `DecrefRegion` at the root's `decref_point`, which post-dominates the
        // child's last use (docs/impl/region/merging.md, gate 6 — a child never
        // outlives its parent). After `static_slot` canonicalization the child's
        // own decref would name the same root slot, and free the shared region
        // under the still-live parent. With no merge, `merged_root` is the
        // identity.
        if info.suppressed_decref_regions.contains(&r)
            || info.owned_group_members.contains(&r)
            || info.merged_root(r) != r
        {
            return ReleaseRoute::Elsewhere;
        }
        let Some(&slot) = self.region_to_slot.get(&r) else {
            return ReleaseRoute::Unslotted;
        };
        let cell = info.cell_release_regions.contains(&r);
        // "A mutated slot is not a release route" (docs/impl/region/bindings.md).
        // The slot belongs to a reassigned binding, so by this region's
        // `decref_point` it holds whatever was last assigned, not the value whose
        // region the release means. Loading it would free THAT live value
        // (tests/impl/region-mutable-reassign-flow.lisp pins it). Two
        // exemptions: a capture-cell placeholder, whose release names the box,
        // and a value a counted module-scope cell stores (`cell_stored_regions`).
        // That value's route is the producer's own ANF slot rather than the
        // cell's, and the cell's counted reference makes the release safe there.
        if info.mutated_binding_value_regions.contains(&r)
            && !cell
            && !info.cell_stored_regions.contains(&r)
        {
            return ReleaseRoute::Refused(Refusal::Mutated);
        }
        // A fn-local reassigned mutable binding owns this slot for its whole
        // scope (`allocate_slot` never reuses a slot), so the slot holds the
        // binding's OWN live value. `region_to_slot[r]` names it only because
        // `record_region_slot` recorded the binding's init region against it.
        // For an immediate-valued counter (`(assign ii (%add ii 1))`) that region
        // is spurious, and its `decref_point` sits inside the loop, so a
        // load+decref+nil-stamp would zero the counter before its increment
        // reads it. A heap accumulator's producer (`(assign acc (f acc))`) is an
        // ANF temp with a slot of its own, whose release still runs (pinned by
        // `tests/impl/region-tailcall-arg-transfer.lisp`, the
        // `tests/impl/region-mutable-reassign-*.lisp` files and
        // `tests/impl/region-capture-cell-loop-uaf.lisp`).
        // `reassigned_local_slots` never records an env cell.
        if self.reassigned_local_slots.contains(&slot.index()) && !cell {
            return ReleaseRoute::Refused(Refusal::Reassigned);
        }
        if cell {
            return ReleaseRoute::Cell(slot.index());
        }
        // A transferred-returned-subtree consumer site: the adopt consumes the
        // result region's whole count, the returned cycle's stuck back-edge
        // reference included, and the activation owner node's completion
        // release set-drops root and members (docs/impl/region/owner.md).
        if info.transfer_adopt_regions.contains(&r) {
            ReleaseRoute::Adopt(slot)
        } else {
            ReleaseRoute::Value(slot)
        }
    }
}
