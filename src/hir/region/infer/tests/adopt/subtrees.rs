// audited: 2026-11-09
//! Pins adopt-edge emission for combined subtrees: store, capture and deep nesting threaded through one owner assignment.
//!
//! docs/impl/region/adopt.md
//!
//! The structural pins exercise each emit mode in isolation (a flat or deep store
//! star, a lone local capture). These probe the modes COMBINED in one
//! externally-unique subtree, where `compute_adopt_edges`'s owner assignment must
//! thread store edges and re-derived capture edges through the same `containers_of`
//! graph. Each assertion is written from the spec ("Both feed owner assignment … each
//! member adopted by its actual parent") — a member's owner is the root when it has a
//! direct root edge, else its unique interior container, whether the edge is a store
//! or a capture.

use super::structural::sole_cell_and_capture_adopts;
use super::*;

/// All `%pair` site regions in `hir` (the seed shapes have one; the two-capture shape has
/// two), in program order. A local generalization of [`sole_pair_region`] for the
/// multi-pair probes below.
fn all_pair_regions(hir: &Hir, info: &RegionInfo) -> Vec<Region> {
    let mut pairs = Vec::new();
    find_all_pairs_helper(hir, &mut pairs);
    pairs
        .iter()
        .filter_map(|id| info.alloc_region.get(id).copied())
        .collect()
}

#[test]
fn adopt_edges_chains_store_and_capture_in_one_subtree() {
    // Combined probe: a Fresh container `root` holds a local capturing closure `c`, and
    // `c` captures the pair `p`. One externally-unique Owned subtree {root, c, p} chains
    // the two emit modes — `c` is adopted by `root` through a STORE edge
    // (`%array-push root c` funnel-records the containment edge `root ⊇ c`),
    // and `p` is adopted by `c` through a re-derived CAPTURE edge `c ⊇ p`. The owner
    // assignment must thread both kinds: `c`'s actual parent is the root (direct store
    // edge), `p`'s actual parent is the non-root container `c`. The root's recursive
    // subtree drop then reclaims the whole chain.
    //
    // `(c)` is called BEFORE the push so the root (last used at the push) post-dominates
    // the closure's last use — the lifetime obligation `member_dp ≤ root_dp` an
    // owner-dies-before-member order would (correctly) refuse.
    //
    // Spec (the actual-parent rule in `compute_adopt_edges`): store adopt (c→root),
    // capture adopt (p→c); each member rides exactly one map.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) root (@array)] \
                  (let [c (fn [] (%first p))] \
                    (begin (c) (%array-push root c) nil))) \
                nil)",
    );
    let p = sole_pair_region(&hir, &info);
    let c = sole_closure_region(&hir, &info);
    // The container root: the target of the funnel containment edge whose source is `c`.
    let root = info
        .containment_edges
        .iter()
        .find(|(_, src, _)| *src == c)
        .map(|&(_, _, dst)| dst)
        .expect("precondition: a store edge root ⊇ c");
    let store: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    let capture: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert!(
        store.contains(&(c, root)),
        "the closure r{} must be adopted by the container root r{} via a STORE edge; \
         store={:?}, capture={:?}",
        c.0,
        root.0,
        store,
        capture,
    );
    assert!(
        capture.contains(&(p, c)),
        "the captured pair r{} must be adopted by its closure r{} via a CAPTURE edge; \
         store={:?}, capture={:?}",
        p.0,
        c.0,
        store,
        capture,
    );
    // Each member rides exactly one map — `p` via capture only, `c` via store only.
    assert!(
        !store.iter().any(|&(m, _)| m == p),
        "the captured pair r{} must not also ride the store map; store={:?}",
        p.0,
        store,
    );
    assert!(
        !capture.iter().any(|&(m, _)| m == c),
        "the stored closure r{} must not also ride the capture map; capture={:?}",
        c.0,
        capture,
    );
}

#[test]
fn adopt_edges_chains_deep_nesting_with_capture_leaf() {
    // Combined probe: deep store nesting with a CAPTURE at the leaf. `root ⊇ mid` and
    // `mid ⊇ c` are store edges, `c ⊇ p` a capture edge — so the subtree {root, mid, c, p}
    // mixes two store levels and a capture in one chain. Each member is adopted by its
    // ACTUAL parent: `mid` by the root, `c` by its interior container `mid`, `p` by its
    // capturing closure `c`. The root's recursive subtree drop walks `owned_children`
    // three levels deep.
    //
    // Spec: store adopts {(mid→root), (c→mid)}; capture adopt {(p→c)}.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) mid (@array) root (@array)] \
                  (let [c (fn [] (%first p))] \
                    (begin (c) (%array-push mid c) (%array-push root mid) nil))) \
                nil)",
    );
    let p = sole_pair_region(&hir, &info);
    let c = sole_closure_region(&hir, &info);
    // mid = the target of the funnel containment edge sourced at `c`; root = the target
    // of the one sourced at `mid`.
    let store_target_of = |src: Region| -> Region {
        info.containment_edges
            .iter()
            .find(|(_, s, _)| *s == src)
            .map(|&(_, _, dst)| dst)
            .unwrap_or_else(|| panic!("precondition: a store edge sourced at r{}", src.0))
    };
    let mid = store_target_of(c);
    let root = store_target_of(mid);
    let store: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    let capture: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert_eq!(
        store,
        [(mid, root), (c, mid)].into_iter().collect(),
        "deep nesting + capture must adopt `mid` by the root r{} and `c` by its actual \
         parent `mid` r{} via STORE edges (root r{}, mid r{}, c r{}); store={:?}",
        root.0,
        mid.0,
        root.0,
        mid.0,
        c.0,
        store,
    );
    assert!(
        capture.contains(&(p, c)),
        "the captured leaf pair r{} must be adopted by its closure r{} via a CAPTURE \
         edge; capture={:?}",
        p.0,
        c.0,
        capture,
    );
}

#[test]
fn adopt_edges_claims_two_captures_in_one_closure() {
    // Combined probe: ONE closure captures TWO local values `p` and `q`. The subtree
    // {c, p, q} is rooted at the closure `c` (the top container — no store edge holds it),
    // and BOTH captured pairs are adopted by `c` through capture edges. Exercises the
    // owner-assignment loop over multiple capture members sharing one owner.
    //
    // Spec: capture adopts {(p→c), (q→c)}; no store adopts.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) q (%pair 3 4)] \
                  (let [c (fn [] (begin (%first p) (%first q)))] (c))) \
                nil)",
    );
    let pairs = all_pair_regions(&hir, &info);
    assert_eq!(
        pairs.len(),
        2,
        "precondition: two %pair members; got {:?}",
        pairs
    );
    let c = sole_closure_region(&hir, &info);
    let capture: rustc_hash::FxHashSet<(Region, Region)> =
        edges.capture.values().flatten().copied().collect();
    for &pr in &pairs {
        assert!(
            capture.contains(&(pr, c)),
            "captured pair r{} must be adopted by the closure root r{} via a CAPTURE \
             edge; capture={:?}",
            pr.0,
            c.0,
            capture,
        );
    }
    let store: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        store.is_empty(),
        "a pure two-capture subtree has no store edges, so no store adopts; got {:?}",
        store,
    );
}

#[test]
fn adopt_edges_capture_root_with_store_child() {
    // Combined probe (mirror of `adopt_edges_chains_store_and_capture_in_one_subtree`):
    // the CLOSURE is the root and the store subtree hangs beneath it. A closure `c`
    // captures a CONTAINER `m`, and `m` holds a value `v`: `c ⊇ m` (capture), `m ⊇ v`
    // (store). So `m` is a capture-adopt member that is itself a store-adopt PARENT.
    //
    // Spec: capture adopt (m→c); store adopt (v→m).
    let (hir, info, edges) = adopt_edges(
        "(begin (let [v (@array) m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m v) (c) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    let capture: rustc_hash::FxHashSet<(Region, Region)> =
        edges.capture.values().flatten().copied().collect();
    // `m` is the container captured by `c` (the capture child whose owner is `c`).
    let m = capture
        .iter()
        .find(|(_, o)| *o == c)
        .map(|&(child, _)| child)
        .unwrap_or_else(|| {
            panic!(
                "the closure r{} must capture-adopt the container m; capture={:?}",
                c.0, capture
            )
        });
    // `v` is the store child of `m`.
    let store: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    let v = store
        .iter()
        .find(|(_, owner)| *owner == m)
        .map(|&(child, _)| child)
        .unwrap_or_else(|| {
            panic!(
                "the container m r{} must store-adopt its value v; store={:?}",
                m.0, store
            )
        });
    assert_eq!(
        capture,
        [(m, c)].into_iter().collect(),
        "the container m r{} must be capture-adopted by the closure root c r{}, and nothing \
         else; capture={:?}",
        m.0,
        c.0,
        capture,
    );
    assert_eq!(
        store,
        [(v, m)].into_iter().collect(),
        "the value v r{} must be store-adopted by its container m r{}, and nothing else; \
         store={:?}",
        v.0,
        m.0,
        store,
    );
}

/// An inline a↔b cycle whose root `x` is stored into a holder `keep`, which dies at
/// the store. The transfer cut refuses a stored consumer, and the holder owns the
/// cycle instead: `keep` adopts `a`, and `a` adopts `b`.
///
/// Counter-factual: with `x`'s init held to its binder node, `a` outlived `keep`'s
/// drop, the holder refused the subtree, and the cycle leaked two regions per run.
#[test]
fn a_stored_inline_cycle_is_owned_by_its_holder() {
    let (_, info) = analyze_full(
        "(begin (let [keep (@array)] \
           (let [x (let [a (@array) b (@array)] \
             (begin (%array-push a b) (%array-push b a) a))] \
             (begin (%array-push keep x) nil))) \
         nil)",
    );
    assert!(
        info.transfer_adopt_regions.is_empty(),
        "a stored consumer refuses the transfer cut; got {:?}",
        info.transfer_adopt_regions,
    );
    let dsts: rustc_hash::FxHashSet<Region> =
        info.containment_edges.iter().map(|&(_, _, d)| d).collect();
    let srcs: rustc_hash::FxHashSet<Region> =
        info.containment_edges.iter().map(|&(_, s, _)| s).collect();
    let keep = *dsts
        .iter()
        .find(|r| !srcs.contains(r))
        .expect("keep is a target only");
    let adopts: rustc_hash::FxHashSet<(Region, Region)> =
        info.owned_adopt_edges.values().flatten().copied().collect();
    let a = adopts
        .iter()
        .find(|&&(_, o)| o == keep)
        .map(|&(m, _)| m)
        .unwrap_or_else(|| panic!("keep r{} adopts the root a; got {adopts:?}", keep.0));
    let b = *srcs
        .iter()
        .find(|&&r| r != a && dsts.contains(&r))
        .expect("b, the other cycle member");
    assert_eq!(
        adopts,
        [(a, keep), (b, a)].into_iter().collect(),
        "keep r{} adopts a r{}, and a adopts b r{}",
        keep.0,
        a.0,
        b.0,
    );
}

/// `v` in a `let` body, captured by `thunk` and released through its cell, with no
/// read of `v` after the spawn. The routed release lands at the spawn, where the
/// closure dies, so the closure's drop post-dominates it and the cell keeps its
/// capture adopt. The lowerer releases the routed value ahead of the cell, and the
/// cell ahead of its owner.
///
/// Counter-factual: with the routed release held to the end of `(def r …)`, past the
/// spawn, the obligation refused the cell, and it was released on its own.
#[test]
fn cell_routed_release_at_the_spawn_keeps_the_adopt() {
    let (_hir, info, edges) = adopt_edges(
        "(let [] (def v (array 1 2 3)) \
                 (def r (let [thunk (fn [] (first v))] \
                          (let [h (sys/spawn-vm thunk)] (sys/thread-state h)))))",
    );
    let (cell, adopts) = sole_cell_and_capture_adopts(&info, &edges);
    assert!(
        adopts.iter().any(|&(member, _)| member == cell),
        "the cell r{} is adopted by the closure that captures it; capture adopts={:?}",
        cell.0,
        adopts,
    );
}
