use super::*;

// ── merge seed: the builder-idiom child→parent merge ──────────────────
//
// docs/impl/region/merging.md § Merging. A freshly-built child aggregate stored
// into the parent `%pair` it becomes a field of merges into the parent's region
// (the down-payment on the forest's owned-subtree drop), but ONLY when the child
// is fresh, sole-held, non-escaping, stored solely into that parent, and dies at
// the same `decref_point`. These pins are written from that predicate: the
// positive merges the canonical nested `%pair`; the negatives are counterfactuals
// proven to bite a deliberately-greedy merge (each fails exactly one gate while
// the others pass). The analysis is additive — it never changes emission — so it
// is exercised through `analyze`/`pipeline`'s `RegionInfo::merged_parent` directly.

#[test]
fn merge_seed_merges_in_loop_nested_literal() {
    // The builder-idiom merge and the ownership-forest adopt share one structural
    // post-dominance predicate (`region::infer::postdom`), but discharge its loop clause
    // differently. Gate 6 passes `EmitMode::Merge`, which WAIVES the loop-enclosure
    // refusal: gates 1+4 make the child reachable solely through the parent
    // (containment), so a loop rebuilding the parent rebuilds the only path to the child —
    // no cross-iteration re-deref, and the merge SHOULD still fire (the
    // bounded-per-iteration arena reclaim is exactly why the merge exists). A fresh nested
    // `%pair` built inside a `while` body must therefore still merge child into parent.
    //
    // This locks the containment distinction: the *same* in-loop shape, were it a
    // store-adopted member (`EmitMode::Adopt`,
    // `adopt::adopt_edges_refuses_loop_enclosed_member`), is REFUSED. If gate 6 ever used
    // the adopt-flavoured predicate (loop guard on), this pin goes RED.
    let (hir, _, info) = pipeline("(let [c 1] (while c (begin (%pair (%pair 1 2) 3) nil)))");
    let edges = pair_store_edges(&hir, &info);
    assert_eq!(
        edges.len(),
        1,
        "the nested pair has exactly one car/cdr store edge (child→parent); got {:?}",
        edges
    );
    let (child, parent) = edges[0];
    assert_eq!(
        info.merged_parent.get(&child),
        Some(&parent),
        "an in-loop fresh nested literal must still merge child r{} into parent r{} \
         (EmitMode::Merge waives the loop clause); merged_parent={:?}",
        child.0,
        parent.0,
        info.merged_parent,
    );
}

#[test]
fn merge_seed_merges_builder_idiom_pair() {
    // The canonical builder idiom: a fresh inner pair stored as the car of a
    // fresh outer pair, the whole nested literal DISCARDED (begin statement) so
    // child and parent die together at the same decref_point. The child's region
    // must merge into the parent's (merged_parent[child] == parent).
    let (hir, _, info) = pipeline("(begin (%pair (%pair 1 2) 3) nil)");
    let edges = pair_store_edges(&hir, &info);
    assert_eq!(
        edges.len(),
        1,
        "the nested pair has exactly one car/cdr store edge (child→parent); got {:?}",
        edges
    );
    let (child, parent) = edges[0];
    assert_eq!(
        info.merged_parent.get(&child),
        Some(&parent),
        "the fresh, sole-held, non-escaping, together-dying child pair r{} must \
         merge into the parent pair r{} it is stored into; merged_parent={:?}",
        child.0,
        parent.0,
        info.merged_parent,
    );
}

#[test]
fn merge_seed_refuses_escaping_child() {
    // The child pair is stored into the parent AND then returned (the begin's
    // tail is `inner`). A returned child outlives the parent's free, so the merge
    // must refuse — eliminating its store edge after a merge would free it with
    // the parent while the caller still holds it.
    let (hir, _, info) =
        pipeline("(let [inner (%pair 1 2)] (begin (%first (%pair inner 8)) inner))");
    let edges = pair_store_edges(&hir, &info);
    assert!(
        !edges.is_empty(),
        "precondition: a child→parent pair-store edge must exist"
    );
    for (child, _parent) in edges {
        assert!(
            !info.merged_parent.contains_key(&child),
            "an escaping (returned) child r{} must not be merged; merged_parent={:?}",
            child.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_seed_refuses_non_sole_held_child() {
    // The child pair is stored into TWO different parent pairs (two distinct
    // car/cdr store edges from one source). It is aliased across the two
    // aggregates, so its references must stay independently accounted — merging
    // into one parent would free it under the other.
    let (hir, _, info) = pipeline(
        "(let [inner (%pair 1 2)] \
           (begin (%first (%pair inner 8)) (%first (%pair inner 9))))",
    );
    let edges = pair_store_edges(&hir, &info);
    // The same child source appears in ≥2 edges with distinct targets.
    let child = edges
        .iter()
        .find(|(c, _)| edges.iter().filter(|(c2, _)| c2 == c).count() >= 2)
        .map(|&(c, _)| c)
        .expect("precondition: a child stored into two distinct parents");
    assert!(
        !info.merged_parent.contains_key(&child),
        "a child r{} stored into two distinct parents must not be merged; merged_parent={:?}",
        child.0,
        info.merged_parent,
    );
}

#[test]
fn merge_seed_refuses_mutable_array_target() {
    // The pair is pushed into a mutable @array, not stored into a fresh
    // immutable aggregate. The push edge targets the collection (a
    // runtime-counted store), not the pair-site's own allocation, so it is not a
    // builder-idiom edge at all — the pushed pair's region merges into nothing.
    let (hir, _, info) = pipeline("(def @acc @[])\n(%array-push acc (%pair 1 2))");
    // The pushed pair's region: the %pair inside the program.
    let mut pairs = Vec::new();
    find_all_pairs_helper(&hir, &mut pairs);
    for pair_id in pairs {
        if let Some(&r) = info.alloc_region.get(&pair_id) {
            assert!(
                !info.merged_parent.contains_key(&r),
                "a pair r{} pushed into a mutable @array (not a fresh immutable \
                 aggregate store) must not be merged; merged_parent={:?}",
                r.0,
                info.merged_parent,
            );
        }
    }
}

#[test]
fn merge_seed_refuses_non_coincident_decref_point() {
    // The child pair is stored into a parent pair (discarded), then READ AGAIN
    // afterward (`(%first inner)`), so the child's decref_point lands past the
    // parent's. The child outlives the parent; merging would free it (with the
    // parent) before the later read. Refused on the coincident-decref_point gate
    // (the mutable-accumulator shape's lifetime mismatch, in miniature).
    let (hir, _, info) =
        pipeline("(let [inner (%pair 1 2)] (begin (%first (%pair inner 9)) (%first inner)))");
    let edges = pair_store_edges(&hir, &info);
    assert!(
        !edges.is_empty(),
        "precondition: a child→parent pair-store edge must exist"
    );
    for (child, parent) in edges {
        // Anchor the counterfactual: the gate that bites here is decref_point,
        // so the child's demise must genuinely land after the parent's.
        let cdp = info.region_data.get(&child).map(|d| d.decref_point);
        let pdp = info.region_data.get(&parent).map(|d| d.decref_point);
        assert_ne!(
            cdp, pdp,
            "precondition: child and parent decref_points must differ for this pin"
        );
        assert!(
            !info.merged_parent.contains_key(&child),
            "a child r{} that outlives its parent r{} (non-coincident decref_point) \
             must not be merged; merged_parent={:?}",
            child.0,
            parent.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_seed_is_a_forest_root_collapses_three_levels() {
    // A three-deep fresh nested literal, discarded: every level's region merges
    // up, so `merged_root` of the innermost collapses to the outermost (the
    // down-payment on owned-subtree drop — the whole literal becomes one region).
    let (hir, _, info) = pipeline("(begin (%pair (%pair (%pair 1 2) 3) 4) nil)");
    let edges = pair_store_edges(&hir, &info);
    // Find the outermost parent: the one that is never itself a child.
    let children: rustc_hash::FxHashSet<Region> = edges.iter().map(|&(c, _)| c).collect();
    let root = edges
        .iter()
        .map(|&(_, p)| p)
        .find(|p| !children.contains(p))
        .expect("an outermost parent that is never a child");
    for &(child, _) in &edges {
        assert_eq!(
            info.merged_root(child),
            root,
            "every level of a fresh nested literal must collapse to the outermost \
             region r{}; merged_root(r{}) disagreed; merged_parent={:?}",
            root.0,
            child.0,
            info.merged_parent,
        );
    }
}
