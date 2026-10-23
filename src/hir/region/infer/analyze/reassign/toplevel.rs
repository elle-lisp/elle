// audited: 2026-10-20
//! The module-scope half of the 1-slot-container model: a file-letrec cell
//! that adopts a fresh allocation and counts a call result's store.
//!
//! docs/impl/region/bindings.md

use super::super::super::*;
use super::chain::ReassignSites;
use crate::hir::region::CellContainer;

/// Apply the model to every module-scope (file-letrec) reassigned binding.
///
/// The cell adopts the producer reference for the init and for every stored
/// fresh allocation, so the gate is sole-held over ALL of its regions; the
/// final, never-overwritten value is released by the file-letrec frame
/// teardown rather than by a content drop of its own. A stored CALL RESULT is
/// counted instead: the store increfs and the producer's release stays, pinned
/// to the store site by the container the cell then records.
pub(super) fn apply_module_scope(
    info: &mut RegionInfo,
    top_level_reassigns: &ReassignSites,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    escape_info: &crate::hir::EscapeInfo,
    sole_held: &dyn Fn(Binding, Region) -> bool,
) {
    for (b, stores) in top_level_reassigns {
        let regions = stores.value_region_set();
        let init_regions = inference_binding_regions
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        // The mutated-slot backstop (docs/impl/region/bindings.md), recorded
        // UNCONDITIONALLY — before the gate. A
        // top-level reassigned binding's slot is overwritten over time, so a
        // value-routed release of ANY region it holds through that slot loads
        // whatever the slot holds THEN and frees it, not the region intended
        // (region-mutable-reassign-flow facet 3). The lowerer skips the value
        // route for any region here, with one exemption: a value a counted
        // cell stores (`cell_stored_regions`, recorded below), whose route is
        // the producer's own ANF slot. Fn-local reassigns are NOT recorded:
        // their final value's release IS a legitimate scope-exit slot route
        // (region-tailcall-arg-transfer).
        for &r in init_regions.iter().chain(regions.iter()) {
            info.mutated_binding_value_regions.insert(r);
        }
        // The not-returned check reads `EscapeInfo`'s return facet, per
        // binding: a return transfers the value's reference to the caller,
        // which the cell also claims — two static owners of one reference. A
        // runtime-counted escape (a container store, a capture, an opaque-call
        // clique, value-succession into the cell's own next value) is NOT
        // refused; the `reassign_gate_keeps_*` tests pin that boundary.
        // `binding_source_regions` is "where the value points", not "where it
        // lives", so projecting a returned-region set onto the cell's regions
        // would be unsound. Guarded by "the cell carries a heap region": an
        // immediate-valued cell read in tail position is "returned" but
        // carries no reference to transfer.
        let has_heap_region = !init_regions.is_empty() || !regions.is_empty();
        let returned = has_heap_region && escape_info.binding_escapes_via_return(*b);
        let all_sole = !returned
            && init_regions
                .iter()
                .chain(regions.iter())
                .all(|&r| sole_held(*b, r));
        if !all_sole {
            continue;
        }
        // A fresh allocation is DONATED: its ordinary decref is suppressed and
        // drop-on-overwrite is that single reference's one release, so
        // `lower_assign` emits no incref-on-store (`donated_overwrite_sites`).
        // A CALL RESULT is COUNTED, as fn-local content is: the stored region's
        // decref is the producer's release, pinned to the store site by the
        // container recorded below, and the cell's reference is the store's
        // incref, released by drop-on-overwrite. The call result's region is a
        // runtime fact the cell cannot claim by name, which is why it counts
        // (docs/impl/region/bindings.md).
        let donates = !regions.iter().any(|r| info.call_result_regions.contains(r));
        for s in stores.sites() {
            info.drop_on_overwrite_sites.insert(s);
            if donates {
                info.donated_overwrite_sites.insert(s);
            }
        }
        // The init is stored uncounted at the define, so its decref is
        // suppressed either way; a donated store suppresses its value regions
        // too. A counted store keeps them, and records the container that
        // carries the store-site pin and the binding-chain hold-back
        // (`decref::populate_decref_points`). The seed is the last store; the
        // placement passes move it to the cell's last access, and only the
        // uncounted-read and return extensions read it from a module-scope
        // cell, which emits no content drop.
        let stored: Vec<Region> = stores.value_regions().collect();
        for &r in init_regions {
            if donates || !stored.contains(&r) {
                info.suppressed_decref_regions.insert(r);
            }
        }
        if donates {
            info.suppressed_decref_regions
                .extend(regions.iter().copied());
        } else if let Some(seed) = stores.sites().last() {
            let cell = CellContainer::module_scope(stores.clone(), seed);
            info.cell_stored_regions.extend(cell.stores.value_regions());
            info.cell_containers.insert(*b, cell);
        }
    }
}
