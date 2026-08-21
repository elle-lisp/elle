use super::*;

// ── ownership inference: a `Fresh` native's embed declaration ────────────────────
//
// A `Fresh` native whose result EMBEDS an argument declares which args it embeds
// (`PrimitiveDef::embeds`, region/effects.md § "Native region effects"). The walk's
// `Fresh` arm then records `result ⊇ arg` in `containment_edges` — the compile-time
// analog of the runtime alloc-scan (`find_object_cross_refs`) that counts the same
// embedding. Without it the forest cannot see a captured value flow OUT through an
// escaping result, so it wrongly folds the value into the capturing closure's Owned
// subtree. `with-traits` is the canonical embedder: it clones its arg-0 value with the
// arg-1 struct attached as the `traits` side-field, so it embeds arg 1 (`embeds: &[1]`).

/// The single region captured by the lambda whose `alloc_region` is `closure` — its
/// sole capture's binding's sole source region. The embed-declaration probe's shape
/// has exactly one capture (the trait table); panics otherwise.
fn sole_captured_region(hir: &Hir, info: &RegionInfo, closure: Region) -> Region {
    fn walk(h: &Hir, info: &RegionInfo, closure: Region, out: &mut Vec<Region>) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure) {
                for c in captures {
                    if let Some(rs) = info.binding_source_regions.get(&c.binding) {
                        out.extend(rs.iter().copied().filter(|&r| r != closure));
                    }
                }
            }
        }
        h.for_each_child(|c| walk(c, info, closure, out));
    }
    let mut out = Vec::new();
    walk(hir, info, closure, &mut out);
    out.sort_by_key(|r| r.0);
    out.dedup();
    assert_eq!(
        out.len(),
        1,
        "shape must have exactly one captured region; got {out:?}",
    );
    out[0]
}

#[test]
fn with_traits_embed_refuses_adopt_of_captured_escaping_table() {
    // Fixture shape (tests/integration/fixtures/region-traits-capture-adopt-uaf.lisp):
    // a closure `make` CAPTURES a struct `shared-tbl` and, in its body, attaches it as a
    // trait table with `with-traits`. The traited RESULT escapes `make` (returned from
    // its body) with its `traits` side-field still referencing the captured table.
    //
    // `with-traits` is a `Fresh` NATIVE that embeds arg 1 (the table) into the fresh
    // result's `traits` side-field — declared by `PrimitiveDef::embeds = &[1]`. The walk
    // records the containment edge `result ⊇ table`, so external uniqueness sees the
    // table referenced from OUTSIDE make's subtree (by the escaping result region) and
    // REFUSES to fold it in: the table stays Shared (per-region RC), reclaimed under the
    // live result's reference.
    //
    // Counterfactual (RED before the embed declaration): with no `result ⊇ table` edge
    // the forest judged the captured table externally unique to `make`, capture-adopted
    // it, and make's subtree drop freed it under the escaped result's `traits` field — a
    // use-after-free (`UpdateCapture` under `--trace=guardfree`; the fixture's SIGSEGV).
    let src = "(begin (let [shared-tbl {:type :my-type}] \
                        (let [make (fn (data) (with-traits [data] shared-tbl))] \
                          (make 1))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    let make_r = sole_closure_region(&hir, &info);
    let tbl = sole_captured_region(&hir, &info, make_r);
    // Precondition: `make` genuinely captures the table (so absent the embed edge the
    // forest would fold it into make's Owned subtree — the counterfactual's premise).
    assert!(
        closure_captures_region(&hir, &info, tbl, make_r),
        "precondition: the closure r{} must capture the table r{}",
        make_r.0,
        tbl.0,
    );
    // The invariant: the captured table, embedded into an escaping result, is adopted by
    // NOBODY — it stays Shared (per-region RC). Asserted FIRST so the counterfactual
    // fails here (the table IS capture-adopted before the fix), on the real defect.
    let adopts: Vec<(Region, Region)> = edges
        .store
        .values()
        .chain(edges.capture.values())
        .flatten()
        .copied()
        .collect();
    assert!(
        !adopts.iter().any(|&(m, _)| m == tbl),
        "the captured table r{} embedded into an escaping result must NOT be adopted \
         (it stays Shared) — got adopts {:?}",
        tbl.0,
        adopts,
    );
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, tbl),
        "the captured-and-embedded table r{} must be in no Owned subtree; owned={:?}",
        tbl.0,
        owned,
    );
    // The mechanism: the fix records the with-traits FRESH result ⊇ the table region.
    let (_, embed_src, result) = info
        .containment_edges
        .iter()
        .copied()
        .find(|&(_, src, _)| src == tbl)
        .unwrap_or_else(|| {
            panic!(
                "with-traits (Fresh, embeds arg 1) must record `result ⊇ table` for the \
                 captured table r{}; containment={:?}",
                tbl.0, info.containment_edges,
            )
        });
    assert_eq!(embed_src, tbl, "the embed's contained member is the table");
    assert_ne!(
        result, tbl,
        "the embed's container is the with-traits result"
    );
    assert!(
        info.fresh_result_regions.contains(&result),
        "the embed container r{} is the with-traits FRESH result",
        result.0,
    );
}

// ── ownership inference: combined store + capture + deep-nesting subtrees ───────
//
// The pins above exercise each emit mode in isolation (a flat/deep store star, a lone
// local capture). These probe the modes COMBINED in one externally-unique subtree, where
// `compute_adopt_edges`'s owner assignment must thread store edges and re-derived capture
// edges through the same `containers_of` graph. Each assertion is written from the spec
// ("Both feed owner assignment … each member adopted by its actual parent") — a member's
// owner is the root when it has a direct root edge, else its unique interior container,
// regardless of whether the edge is a store or a capture.

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
    // Spec (compute_adopt_edges doc, "Both feed owner assignment"): store adopt (c→root),
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
fn adopt_edges_refuses_nested_capture_on_lifetime_overextension() {
    // Boundary pin + step-3 frontier (NOT a bug — a sound conservative refusal). A closure
    // capturing a CLOSURE capturing a value — `c1 ⊇ c2 ⊇ p`, all CAPTURE edges, the shape a
    // closure-web is built from. `compute_owned_subtrees` ADMITS {c1, c2, p} as externally
    // unique (nothing escapes), but `compute_adopt_edges` REFUSES it on the lifetime
    // obligation, so it stays Shared (the always-legal baseline) — no adopt is emitted.
    //
    // Why it refuses: the tight last-use (`binding_last_use`) bounds a captured value by its
    // owning closure's last CALL. For a NESTED capture the owning closure
    // is itself captured, so its "last call" is already the over-extended one-step-past
    // position, and the bound cascades one structural step PER nesting level. Measured here:
    // the root `c1` dies at its `decref_point`, but `p`'s tight last-use lands one step LATER
    // (`p`'s owner `c2` is captured by `c1`, so `c2`'s last-call bound is already at `c1`'s
    // death, and `p` inherits one step past that). The obligation `member_dp ≤ root_dp` then
    // cannot prove `p` dies before the root, so it refuses — sound, since adopting would risk
    // freeing `p` under a still-live reference the analysis cannot rule out.
    //
    // At runtime `p` IS dead when `(c1)` returns (c1/c2/p all discarded together), so a
    // tighter transitive-through-capture last-use would let the forest claim this — that
    // improvement is part of the cross-fiber owner = activation cut (which claims the
    // closure-web). When it lands, this pin flips and forces the author to
    // confirm the adopt then emits. Until then the refusal is the correct boundary.
    let src = "(begin (let [p (%pair 1 2)] \
                        (let [c2 (fn [] (%first p))] \
                          (let [c1 (fn [] (c2))] (c1)))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    let p = sole_pair_region(&hir, &info);
    // (1) The subtree is admitted as externally unique — the refusal is at the adopt stage,
    // not the subtree stage (so a fix lands in `compute_adopt_edges`/the last-use, not the
    // external-uniqueness walk).
    let (_, _, owned) = owned_subtrees_with_effects(src);
    let root = owned
        .iter()
        .find(|(_, members)| members.contains(&p))
        .map(|(&r, _)| r)
        .expect("compute_owned_subtrees must admit the externally-unique {c1, c2, p} subtree");
    // (2) Pin the REASON: `p`'s tight last-use over-extends past the root's decref_point —
    // the exact condition the lifetime obligation refuses on. A tighter last-use that lowers
    // `p` to ≤ root would flip both this and the emptiness assertion below.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&root)
        .expect("root has region_data")
        .decref_point);
    let p_tight = info
        .binding_last_use
        .get(&p)
        .map(|&id| ord(id))
        .expect("the captured pair has a tight last-use");
    assert!(
        p_tight > root_dp,
        "precondition (the refusal cause): the nested-captured pair r{}'s tight last-use \
         ({p_tight}) must over-extend past the subtree root r{}'s decref_point ({root_dp}); \
         if it no longer does, the lifetime obligation now admits the chain and the \
         emptiness assertion below must be replaced by the positive adopt expectation",
        p.0,
        root.0,
    );
    // (3) So no adopt is emitted for the whole program — the chain stays Shared.
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the nested-capture chain must emit NO adopts (refused to Shared on the lifetime \
         obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
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

#[test]
fn adopt_edges_refuses_captured_store_member_on_lifetime() {
    // Boundary pin + UAF regression (NOT a bug — a sound conservative refusal, the
    // owner-aware lifetime obligation). A two-level subtree whose interior cycle back-edge
    // is a CAPTURE: `root` holds `m` (store `root ⊇ m`), `m` holds a closure `c` (store
    // `m ⊇ c`), and `c` captures `m` back (capture `c ⊇ m`) — the m↔c reference cycle. The
    // subtree {root, m, c} is admitted as externally unique, and `m`'s owner is the root
    // (its direct store edge, preferred over the capture container `c`).
    //
    // The hazard: `m` is ALSO a captured value (captured by `c`), and a captured value's
    // `decref_point` is over-extended one structural step past its owning closure.
    // `m` is STORE-adopted (owner = root), so its own `DecrefValueRegion`
    // is NOT suppressed — it fires at that over-extended position, AFTER the root's decref.
    // `@array` regions are Fresh call-results released value-based, so the root's drop frees
    // `m`, and `m`'s own later decref-value then `result_region_of`s a freed page: a
    // use-after-free (the original failure this pin guards — `region_ownership` SIGSEGV'd
    // under guardfree, runtime test `region_ownership_capture_back_edge_cycle_runs_sound`).
    //
    // The owner-aware obligation bounds a store-adopted member by its STRUCTURAL
    // `decref_point` (where its live decref fires), not the tight last-use, so `m`'s
    // over-extension refuses the subtree — no REGION root owns it (this refusal is
    // permanent and correct). The m↔c SCC itself is claimed by the ACTIVATION cut
    // instead (`activation_adopts_capture_back_edge_scc`), whose owner-node release
    // post-dominates the whole activation. The `(c)` call is present so `c` is genuinely
    // used; it does not change the refusal.
    let src = "(begin (let [root (@array) m (@array)] \
                        (let [c (fn [] (length m))] \
                          (begin (%array-push m c) (c) (%array-push root m) nil))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    // `m` is the @array that is BOTH a store source (`m → root`) and a store target
    // (`c → m`) among the funnel containment edges; `root` is target-only, `c`
    // source-only.
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(_site, s, d) in &info.containment_edges {
        srcs.insert(s);
        dsts.insert(d);
    }
    let m = *srcs
        .iter()
        .find(|r| dsts.contains(r))
        .expect("precondition: `m` is both a store source and target");
    let root = *dsts
        .iter()
        .find(|r| !srcs.contains(r))
        .expect("precondition: a target-only container root");
    // (1) The subtree is admitted as externally unique — the refusal is at the adopt stage,
    // not the subtree stage.
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        owned.values().any(|s| s.contains(&m) && s.contains(&root)),
        "compute_owned_subtrees must admit the externally-unique {{root, m, c}} subtree \
         (root r{}, m r{}); owned={:?}",
        root.0,
        m.0,
        owned,
    );
    // (2) Pin the REASON: `m`'s STRUCTURAL decref_point (where its live store-member
    // decref-value fires) over-extends past the root's — the exact condition the owner-aware
    // obligation refuses on. A future cut that suppresses or tightens `m`'s decref would flip
    // this, forcing the author to confirm the runtime then reclaims it soundly.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&root)
        .expect("root region_data")
        .decref_point);
    let m_dp = ord(info
        .region_data
        .get(&m)
        .expect("m region_data")
        .decref_point);
    assert!(
        m_dp > root_dp,
        "precondition (the refusal cause): the captured-and-store-owned member r{}'s \
         structural decref_point ({m_dp}) must over-extend past the root r{}'s ({root_dp}); \
         if not, the obligation now admits it and this refusal pin must become a positive \
         reclamation expectation",
        m.0,
        root.0,
    );
    // (3) So the subtree is refused — no adopt emitted, the m↔c cycle stays Shared.
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the captured-store-member subtree must emit NO adopts (refused to Shared on the \
         owner-aware lifetime obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
    );
}
