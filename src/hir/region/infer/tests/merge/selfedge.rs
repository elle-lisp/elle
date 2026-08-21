use super::*;

// ── C5: the self-edge elimination predicate (transform 2) ──────────────
//
// `RegionInfo::is_merge_self_edge(source, target)` — `merged_root(source) ==
// merged_root(target)` — classifies the cross-region store edges the
// builder-idiom merge collapses into intra-region self-edges: the eliminable
// class whose `IncrefRegion` the free-time cascade never balances (it skips a
// region's references into itself, regionpool/introspect.rs `rid != own_id`),
// so keeping it past a merge leaks. The predicate is measure-only in C5
// (emission is unchanged); the C6 flip drops the edges it flags. These pins are
// written from that spec in all three directions, each a counterfactual: the
// builder idiom (and every level of a nested literal) MUST be flagged — else C6
// leaks it, and the naive pre-merge identity test (`source == target`) that can
// never fire for a `record_edge`-recorded edge fails them; an escaping
// `(%pair x x)` alias MUST NOT be flagged (eliminating one of its two
// distinct-region increfs is a UAF); a may-store clique edge MUST NOT be flagged
// (its balancing decref is the target's runtime content scan).

#[test]
fn merge_self_edge_flags_builder_idiom() {
    // The canonical builder idiom: a fresh inner pair stored as the car of a
    // fresh outer pair, discarded. The merge collapses child→parent, so the
    // child→parent store edge is now an intra-region self-edge — exactly the
    // `IncrefRegion` C6 must drop (left in place it leaks: the cascade skips
    // self-references).
    let (hir, _, info) = pipeline("(begin (%pair (%pair 1 2) 3) nil)");
    let edges = pair_store_edges(&hir, &info);
    assert_eq!(
        edges.len(),
        1,
        "the nested pair has exactly one child→parent store edge; got {:?}",
        edges
    );
    let (child, parent) = edges[0];
    // Precondition: the merge actually fired (anchors the counterfactual on the
    // builder idiom, not on an accidental non-merge).
    assert_eq!(
        info.merged_parent.get(&child),
        Some(&parent),
        "precondition: the builder idiom must merge child r{} into parent r{}",
        child.0,
        parent.0,
    );
    assert!(
        info.is_merge_self_edge(child, parent),
        "the merged child→parent store edge (child r{}, parent r{}) must be flagged \
         a self-edge — post-merge both resolve to one region, so its IncrefRegion \
         is unbalanced and C6 must drop it; merged_parent={:?}",
        child.0,
        parent.0,
        info.merged_parent,
    );
}

#[test]
fn merge_self_edge_flags_every_level_of_nested_literal() {
    // A three-deep fresh nested literal, discarded: every level merges up, so
    // EVERY car/cdr store edge is intra-region post-merge — the whole literal
    // collapses to one region, every edge a self-edge C6 drops (the down-payment
    // on owned-subtree drop).
    let (hir, _, info) = pipeline("(begin (%pair (%pair (%pair 1 2) 3) 4) nil)");
    let edges = pair_store_edges(&hir, &info);
    assert!(
        edges.len() >= 2,
        "a three-level nest has ≥2 child→parent store edges; got {:?}",
        edges
    );
    for (src, dst) in edges {
        assert!(
            info.is_merge_self_edge(src, dst),
            "a fully-fresh nested literal collapses to one region, so its store edge \
             (src r{}, dst r{}) must be flagged a self-edge; merged_parent={:?}",
            src.0,
            dst.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_self_edge_refuses_escaping_alias() {
    // `(%pair x x)` where x ESCAPES (the begin returns it): x is not sole-held by
    // the outer pair (it is also returned), so the merge refuses it and the two
    // distinct-region `x→outer` increfs must BOTH stay — the cascade finds two
    // references at free(outer) and decrefs twice, so two increfs are required.
    // Flagging either as a self-edge (and eliminating it in C6) is a UAF.
    let (hir, _, info) = pipeline("(let [x (%pair 1 2)] (begin (%first (%pair x x)) x))");
    let edges = pair_store_edges(&hir, &info);
    // Precondition: the alias shape — the same (src,dst) pair recorded twice
    // (`record_edge` does not dedup; `(%pair x x)` stores x as both car and cdr).
    let has_alias = edges
        .iter()
        .any(|e| edges.iter().filter(|&x| x == e).count() >= 2);
    assert!(
        has_alias,
        "precondition: a repeated x→outer alias edge; got {:?}",
        edges
    );
    for (src, dst) in edges {
        assert!(
            !info.is_merge_self_edge(src, dst),
            "an escaping (%pair x x) alias edge (src r{}, dst r{}) must NOT be flagged \
             a self-edge — x is unmerged, both increfs are required; merged_parent={:?}",
            src.0,
            dst.0,
            info.merged_parent,
        );
    }
}
