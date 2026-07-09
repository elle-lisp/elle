//! The ownership-forest passes.
//!
//! Classify externally-unique Owned subtrees and record their containment edges
//! as adopt sites, then layer the transferred-returned-subtree cut, the
//! co-owned-cycle cut, and the activation-owner cut on top. All unconditional —
//! the ownership forest is how the language runs, not an opt-in dialect (see
//! docs/impl/region/ownership.md § "One semantics, every backend"). Extracted
//! verbatim from `analyze_regions_with`; the inline comments carry the WHY.

// `super` is `hir::regions::analyze`; `super::super` reaches the sibling
// `hir::regions` items (including the `ownership` submodule) the original
// block saw through `use super::*` and `super::ownership`.
use super::super::*;

/// Run the ownership passes, recording adopt edges, transfer regions, owned
/// groups, and activation-adopt sites into `info`.
pub(super) fn apply_ownership(
    info: &mut RegionInfo,
    hir: &Hir,
    escape_info: &crate::hir::EscapeInfo,
    arena: &BindingArena,
    order: &HashMap<HirId, u32>,
    transfer_call_class: &CallClassification,
) {
    // Ownership forest (docs/impl/region/ownership.md § "Adoption and subtree drop").
    // Classify externally-unique Owned subtrees and record their interior
    // containment edges as `AdoptRegion` sites (with the lifetime obligation and
    // merge-overlap filters applied). Runs LAST, after the final `region_data`
    // (its lifetime filter reads `decref_point`) and after the merge seed (so it
    // can exclude merge participants). This is unconditional — the ownership
    // forest is how the language runs, not an opt-in dialect (§ "One semantics,
    // every backend"); a subtree the inference cannot prove externally unique
    // simply stays Shared (the always-legal per-region-RC baseline), so no adopt
    // edge is emitted for it and its emission is the RC baseline by construction.
    let mut adopt =
        super::super::ownership::compute_adopt_edges(hir, info, escape_info, arena, order);
    // The transferred-returned-subtree cut (docs/impl/region/owner.md
    // § "Owner nodes" — "The transferred returned subtree"): a producer's
    // externally-unique returned cycle is owned by its CONSUMING activation. Its
    // interior owner edges merge into the ordinary adopt maps here — BEFORE the
    // capture-suppression loop below, so a transfer capture member rides the same
    // suppress ⊆ adopt contract — and each consumer site's call-result region lands
    // in `transfer_adopt_regions`, whose release the lowerer replaces with
    // `AdoptIntoActivation` (regiondecref.rs). Disjoint from the maps' existing
    // entries by construction: a subtree containing the returned root is refused by
    // the seed-poisoned subtree walk, and a transfer member reached from any outside
    // container fails external uniqueness.
    let transfer = super::super::ownership::compute_transfer_adopts(
        hir,
        info,
        escape_info,
        arena,
        transfer_call_class,
        order,
    );
    for (site, edges) in transfer.store {
        adopt.store.entry(site).or_default().extend(edges);
    }
    for (site, edges) in transfer.capture {
        adopt.capture.entry(site).or_default().extend(edges);
    }
    info.transfer_adopt_regions = transfer.result_regions;
    // A capture-adopted member is reclaimed solely by its closure's subtree drop, so
    // suppress its own compiler decref. This is load-bearing, unlike a STORE-adopted
    // member: the lifetime obligation bounds a store member's `decref_point` at or
    // below the root's drop (its decref hits the still-frozen region — a no-op), but a
    // captured member's `decref_point` is the over-extended structural position one
    // step past the closure (the over-keep the TIGHT obligation admits past), so its
    // unsuppressed decref would fire AFTER the subtree drop freed it — a direct decref
    // of an absent region, tripping the `regionstore` phantom/double-free assert.
    // Collected from `adopt.capture` before the maps move into `info`.
    for edges in adopt.capture.values() {
        for &(member, _closure) in edges {
            info.suppressed_decref_regions.insert(member);
        }
    }
    info.owned_adopt_edges = adopt.store;
    info.capture_adopt_edges = adopt.capture;
    // The cell⊇content adopts are emitted at the cell's own store site (keyed by binding),
    // as `AdoptCellRegion(cell, content)`. The content is store-adopted (its own decref is
    // a frozen no-op under the Owned region), so it is NOT suppressed here; the cell region
    // — a capture-adopted member of `adopt.capture` — already was, above.
    info.cell_content_adopt_bindings = adopt.cell_content.iter().copied().collect();

    // Co-owned-cycle cut: a rootless mutual reference cycle is reclaimed
    // symmetrically as one `FreeRegionGroup` at its collective last use, disjoint
    // from the container-rooted adopt subtrees above. `owned_group_members` is the
    // flat union, the O(1) decref-skip set the lowerer consults.
    let groups =
        super::super::ownership::compute_owned_region_groups(hir, info, escape_info, arena, order);
    info.owned_group_members = groups.values().flatten().copied().collect();
    info.owned_region_groups = groups;

    // The activation-owner cut: a capture-back-edge SCC — a container captured by a
    // closure it holds, the cycle no region root can own — is adopted into the
    // executing activation's owner node and freed by its completion release
    // (docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge SCC").
    // Runs LAST among the ownership passes: its disjointness gate reads the merge,
    // adopt, and group claims above. Each member's own compiler decref is suppressed
    // — the node's release is the members' sole demise (the suppress ⊆ adopt
    // contract) — and every decref-emit site re-checks `suppressed_decref_regions`,
    // so no other release path can reach a member.
    let activation =
        super::super::ownership::compute_activation_adopts(hir, info, escape_info, arena, order);
    for members in activation.values() {
        for &m in members {
            info.suppressed_decref_regions.insert(m);
        }
    }
    info.activation_adopt_sites = activation;
}
