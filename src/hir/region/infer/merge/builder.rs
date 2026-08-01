//! The builder-idiom merge seed — a freshly-built child aggregate merged into the
//! parent `%pair` it is stored into. The soundness is pinned entirely by the
//! six-gate predicate in [`compute_merges`] (docs/impl/region/merging.md § Merging).

use super::super::postdom::{EmitMode, PostDom};
use super::super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// Compute the builder-idiom merge forest over a fully-built `RegionInfo` (its
/// `region_data` decref_points final — the lifetime gate reads them) and the
/// structural execution `order` (so lifetimes compare by execution position, not
/// `HirId` magnitude, which ANF makes meaningless). Returns `child → parent` for
/// every region a builder-idiom merge collapses.
pub(crate) fn compute_merges(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    order: &HashMap<HirId, u32>,
) -> HashMap<Region, Region> {
    // The `%pair` intrinsic nodes — the canonical immutable aggregate-store sites.
    // (Together with `alloc_region[site] == dst` below, this uniquely selects a
    // pair's own car/cdr store edges out of `cross_region_refs`; push/put/set-cell
    // and clique edges target something other than the site's own allocation.)
    let mut pair_sites: FxHashSet<HirId> = FxHashSet::default();
    collect_pair_sites(hir, &mut pair_sites);
    if pair_sites.is_empty() {
        return HashMap::new();
    }

    // Per-source store targets: which regions each source region is stored into.
    // Builds the sole-target check (a child stored into exactly one parent) in
    // O(1) per edge instead of rescanning `cross_region_refs`.
    let mut src_targets: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
    for &(_, src, dst) in &info.cross_region_refs {
        src_targets.entry(src).or_default().insert(dst);
    }

    // Regions that flow to a tail/return (the return frontier) — a returned child
    // outlives its parent's free. Projected from escape's authoritative return
    // verdict (`super::escape`), not a solver-local tail set.
    let returned_regions: FxHashSet<Region> = super::super::escape::return_frontier_regions(
        escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );

    // Region → distinct user holders, via the shared index (`region::infer::holders`).
    // Synthetic ANF producer temps are excluded by the type, so the common builder
    // case — an inner pair bound only to an ANF temp — is sole-held by construction;
    // a region held by two USER bindings is an alias and refused (`len() > 1`
    // below). The merge seed admits any non-synthetic holder, so its eligibility
    // predicate is `true` (the read-eligibility filter is the reassign gate's, not
    // the seed's).
    let region_holders = super::super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );

    // A region is a fresh local immutable aggregate iff it holds a real local
    // allocation and is none of the dynamic classes (call-result placeholder,
    // capture cell, reassign-suppressed, mutated 1-slot-container value). A `%pair`
    // alloc region satisfies this; a runtime-fact region never does.
    let fresh_local = |r: Region| -> bool {
        info.live_regions.contains(&r)
            && !info.call_result_regions.contains(&r)
            && !info.cell_release_regions.contains(&r)
            && !info.suppressed_decref_regions.contains(&r)
            && !info.mutated_binding_value_regions.contains(&r)
    };

    // Refuse the merge when the child's holder either RETURNS or is CAPTURED — two
    // DIFFERENT questions answered by two DIFFERENT authorities, not one "escape" check:
    //
    //  - RETURN is a lifetime question, so it reads escape (the authority):
    //    `binding_escapes_via_return`. We cannot read the full `binding_escapes_activation`
    //    here — storing the child INTO the parent is the builder idiom's own (allowed)
    //    store-escape, which the full set folds in and would wrongly refuse — so the
    //    return facet is the exposed, precise sub-question.
    //
    //  - CAPTURE is a REACHABILITY question, not a lifetime one. The merge's soundness
    //    rests on the child being reachable ONLY through the parent: gates 1+4 make it
    //    sole-STORED, and this clause makes it sole-HELD — a holder that a closure also
    //    captures is a second reachability path the parent's single drop does not own. So
    //    it needs the UNCONDITIONAL "is it captured at all" relation — deliberately NOT
    //    escape's capture FACET, which asks the strictly narrower, CONDITIONAL question
    //    "captured by a closure that itself escapes." A child captured by a non-escaping
    //    closure escapes nothing yet is still doubly held, and the merge must refuse it all
    //    the same. Escape is the authority for "does it outlive its activation"; it is NOT
    //    the authority for "is it uniquely held here" — that is the region forest's own
    //    reachability question, answered by the region capture-graph
    //    (`super::escape::captured_bindings`: the bindings some closure captures, sourced
    //    structurally from the HIR — never the lexical proxy `is_captured` the solver is
    //    locked out of).
    let captured = super::super::escape::captured_bindings(hir);
    let holder_non_escaping = |child: Region| -> bool {
        match region_holders.holders_of(child) {
            // No user holder (an ANF-temp-only child) — nothing to escape.
            None => true,
            // More than one user binding holds it: aliased — refuse.
            Some(hs) if hs.len() > 1 => false,
            Some(hs) => hs
                .iter()
                .all(|&b| !escape.binding_escapes_via_return(b) && !captured.contains(&b)),
        }
    };

    // Structural post-dominance over the scope tree — gate 6's authority. Built
    // once (only reached past the pair-site early-return), queried per edge.
    let dom = PostDom::new(hir, order);

    let mut merged_parent: HashMap<Region, Region> = HashMap::new();
    for &(site, child, parent) in &info.cross_region_refs {
        // (1) An immutable `%pair` car/cdr store: a Pair node whose edge target is
        //     the aggregate freshly allocated AT that site, and not a clique site.
        if child == parent
            || !pair_sites.contains(&site)
            || info.alloc_region.get(&site) != Some(&parent)
            || info.hard_edge_sites.contains(&site)
        {
            continue;
        }
        // (2) parent and (3) child are both fresh local immutable aggregates.
        if !fresh_local(parent) || !fresh_local(child) {
            continue;
        }
        // (4) The child is stored ONLY into this parent (not aliased across
        //     aggregates). Repeated stores into the SAME parent are fine.
        if src_targets
            .get(&child)
            .is_none_or(|ts| ts.len() != 1 || !ts.contains(&parent))
        {
            continue;
        }
        // (5) Neither child nor parent escapes. A returned/captured CHILD outlives
        //     the parent's free. A returned/captured PARENT is sound to merge too
        //     (the child still dies within it) but is the deferred widening — start
        //     narrow with the parent as a genuine LOCAL owner (the discarded /
        //     together-consumed nested literal), widen cut by cut
        //     (docs/impl/region/merging.md § Merging).
        if returned_regions.contains(&child)
            || returned_regions.contains(&parent)
            || !holder_non_escaping(child)
            || !holder_non_escaping(parent)
        {
            continue;
        }
        // (6) The parent's free POST-DOMINATES the child's last use — the single
        //     drop point (the shared region's `DecrefRegion` at the parent's
        //     `decref_point`) must not precede the child's own last *direct* use.
        //     Decided STRUCTURALLY over the scope tree, not by `ord` magnitude
        //     (region/merging.md § Merging, condition 6). `EmitMode::Merge`
        //     waives the loop-enclosure clause: gates 1+4 make the child reachable
        //     only through the parent (containment), so a loop rebuilding the parent
        //     rebuilds the only path to the child — an in-loop nested literal still
        //     merges, no cross-iteration re-deref. A child READ after the parent's
        //     death (the aliased / mutable-accumulator shape) is sequenced after the
        //     parent's free and is refused.
        let (Some(cd), Some(pd)) = (
            info.region_data.get(&child).map(|d| d.lifetime_point),
            info.region_data.get(&parent).map(|d| d.lifetime_point),
        ) else {
            continue;
        };
        if !dom.drop_post_dominates(pd, cd, EmitMode::Merge) {
            continue;
        }
        // The merged shape's numeric shadow: a child that does not outlive its
        // parent does not linearize after it. A debug echo of the structural
        // verdict, never the deciding test.
        #[cfg(debug_assertions)]
        {
            let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
            debug_assert!(
                ord(cd) <= ord(pd),
                "merge admitted child r{} whose decref_point ({}) linearizes after \
                 parent r{}'s ({})",
                child.0,
                ord(cd),
                parent.0,
                ord(pd),
            );
        }
        // All six gates pass. The child is stored into exactly this parent
        // (gate 4), so a prior insert can only have named the same parent —
        // idempotent.
        merged_parent.insert(child, parent);
    }
    merged_parent
}

/// Collect every `%pair` (`IntrinsicOp::Pair`) node's HirId.
fn collect_pair_sites(hir: &Hir, out: &mut FxHashSet<HirId>) {
    if let HirKind::Intrinsic {
        op: crate::hir::expr::IntrinsicOp::Pair,
        ..
    } = &hir.kind
    {
        out.insert(hir.id);
    }
    hir.for_each_child(|c| collect_pair_sites(c, out));
}
