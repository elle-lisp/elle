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
    // post-dominance predicate (`regions::postdom`), but discharge its loop clause
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

// ── The letrec closure-cycle merge ────────────────────────────────────
//
// docs/impl/region/letrec.md § The letrec closure-cycle merge. A `letrec`
// self/mutual recursive closure is a
// capture-cell↔closure cycle: the prebound forward-reference cell holds the closure
// (`StoreCaptureCell`) and the closure captures the cell. Per-region RC cannot
// collect the immutable cycle (region/rules.md Rule 8), but every member is
// static-slot (the closure's `alloc_region`, the cell's `begin_cell_regions`),
// sole-held, and non-escaping — so the merge collapses the whole SCC ∪ its cells
// onto ONE region. The interior cell↔closure references become intra-region (the
// alloc-scan and free-cascade both self-skip same-region refs,
// regionpool/introspect.rs `rid != own_id`), so the cycle frees as one arena with
// one `DecrefRegion`. These pins drive that from the spec: the positive cases
// collapse the SCC onto one `merged_root`; the negative refuses an escaping closure.

/// The `Letrec` node binding a name (`loop`/`ping`) — the cycle's binding scope,
/// whose scope-exit is the tight, RC-safe drop site for the merged arena.
fn letrec_binding_node(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    name: &str,
) -> Option<HirId> {
    fn walk(
        h: &Hir,
        arena: &BindingArena,
        symbols: &SymbolTable,
        name: &str,
        out: &mut Option<HirId>,
    ) {
        if let HirKind::Letrec { bindings, .. } = &h.kind {
            if bindings
                .iter()
                .any(|(b, _)| symbols.name(arena.get(*b).name) == Some(name))
            {
                *out = Some(h.id);
            }
        }
        h.for_each_child(|c| walk(c, arena, symbols, name, out));
    }
    let mut out = None;
    walk(hir, arena, symbols, name, &mut out);
    out
}

/// The closure (`Lambda` `alloc_region`) regions and the pre-allocated capture-cell
/// (`begin_cell_regions`) regions in `hir` — the two member kinds of a letrec
/// closure-cycle merge.
fn letrec_cycle_members(hir: &Hir, info: &RegionInfo) -> (Vec<Region>, Vec<Region>) {
    fn walk(h: &Hir, info: &RegionInfo, out: &mut Vec<Region>) {
        if matches!(h.kind, HirKind::Lambda { .. }) {
            if let Some(&r) = info.alloc_region.get(&h.id) {
                out.push(r);
            }
        }
        h.for_each_child(|c| walk(c, info, out));
    }
    let mut closures = Vec::new();
    walk(hir, info, &mut closures);
    let cells: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flat_map(|v| v.iter().map(|(_, r)| *r))
        .collect();
    (closures, cells)
}

#[test]
fn self_recursive_letrec_is_cell_free_not_merged() {
    // A pure self-recursive `letrec` closure (`loop` references only itself) is
    // CELL-FREE: the self-edge does not mark `loop` captured
    // (`hir/analyze/scopes.rs`), so it has no forward cell and its self-reference
    // resolves to the executing closure (`LoadSelf` / a self-call), never a cell
    // load. There is no cell↔closure cycle for the merge to collapse — the merge is
    // the MUTUAL-recursion instrument now (`merge_collapses_mutual_recursion_*`). So
    // `loop` mints no capture cell and is not a merge member; it is reclaimed by
    // ordinary RC / the tail-call deferred release, RC-identical to a top-level recursive
    // `defn`. This is the region-solver-level counterpart of the runtime
    // `self_recursive_loop_is_cell_free` mint pin.
    let (hir, _, info) = pipeline(
        "(begin (letrec [loop (fn [n] (if (%lt n 1) :done (loop (%sub n 1))))] (loop 3)) nil)",
    );
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    assert_eq!(
        closures.len(),
        1,
        "one closure (loop's lambda); got {closures:?}"
    );
    assert!(
        cells.is_empty(),
        "a pure self-recursive `loop` mints NO forward cell — the self-edge does not \
         mark it captured; got cells {cells:?}",
    );
    let loop_r = closures[0];
    assert!(
        !info.merged_parent.contains_key(&loop_r)
            && !info.merged_parent.values().any(|&p| p == loop_r),
        "a cell-free self-recursive closure r{} has no cell↔closure cycle to merge \
         (the merge is mutual-only); merged_parent={:?}",
        loop_r.0,
        info.merged_parent,
    );
}

#[test]
fn merge_collapses_mutual_recursion_letrec_closure_cycle() {
    // ping <-> pong: two closures whose envs reference each other (immutable cycle),
    // each prebound with a capture cell. The merge must collapse all four members
    // (two closures + two cells) onto ONE region.
    let (hir, _, info) = pipeline(
        "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                         pong (fn [n] (ping n))] \
                  (ping 3)) \
               nil)",
    );
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    assert_eq!(
        closures.len(),
        2,
        "two closures (ping, pong); got {closures:?}"
    );
    assert_eq!(cells.len(), 2, "two prebound capture cells; got {cells:?}");
    let members: Vec<Region> = closures.iter().chain(cells.iter()).copied().collect();
    let roots: rustc_hash::FxHashSet<Region> =
        members.iter().map(|&m| info.merged_root(m)).collect();
    assert_eq!(
        roots.len(),
        1,
        "the ping/pong closures and their cells must collapse onto ONE merged root; \
         closures={closures:?} cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
}

#[test]
fn merge_mutual_recursion_cycle_drops_at_binding_scope_not_enclosing() {
    // PROMPTNESS (docs/impl/region/adopt.md § The lifetime obligation the root
    // carries; the §9 promptness ledger). The merged cycle's single `DecrefRegion`
    // must fire at the cycle's BINDING SCOPE — the `letrec` that prebinds its
    // capture cells — not at that scope's enclosing post-dominator. Exercised on
    // MUTUAL recursion (ping/pong), which is what the merge serves: a pure
    // self-recursive letrec is cell-free and never merged
    // (`self_recursive_letrec_is_cell_free_not_merged`). Each of ping/pong captures
    // the OTHER (a sibling capture that keeps a forward cell), so the SCC ∪ cells
    // collapse onto one arena. The capture cell is keyed by the `letrec` NODE
    // (`begin_cell_regions`), and the enclosing-scope walk records a target's STRICT
    // ancestors, so an allocation-site post-dominator over {lambdas, cell-nodes}
    // resolves to the letrec's PARENT — for a top-level cycle, the program `Begin`,
    // i.e. program teardown. The binding-scope drop frees the cycle right after the
    // letrec body (its true last use). This is the counterfactual for that
    // tightening: it FAILS while the drop sits at the enclosing post-dominator.
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(
        "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                         pong (fn [n] (ping n))] \
                  (ping 3)) \
               nil)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    let members: Vec<Region> = closures.iter().chain(cells.iter()).copied().collect();
    assert!(
        !members.is_empty(),
        "precondition: the mutual cycle has closure+cell members"
    );
    let root = info.merged_root(members[0]);
    for &m in &members {
        assert_eq!(
            info.merged_root(m),
            root,
            "precondition: the cycle collapsed onto one merged root r{}",
            root.0,
        );
    }
    let letrec_id = letrec_binding_node(&hir, &arena, &symbols, "ping")
        .expect("the local letrec binding `ping`");
    let dp = info.region_data.get(&root).map(|d| d.decref_point);
    assert_eq!(
        dp,
        Some(letrec_id),
        "the discarded mutual cycle (root r{}) must drop at its binding-scope letrec \
         @{} (its true last use), not at the enclosing post-dominator (the program \
         Begin — program teardown); decref_point was {:?}",
        root.0,
        letrec_id.0,
        dp,
    );
}

#[test]
fn merge_collapses_in_lambda_mutual_recursion_letrec_closure_cycle() {
    // The IN-LAMBDA mutual cycle — the letrec is a lambda body (the universal
    // recursive-local-helper shape, oracle.lisp `recur-local-mutual`). An immutable,
    // lambda-initialized letrec binding's forward cell is a compiled static-slot cell
    // in every position (`BindingInner::letrec_compiled_cell`), so the merge collapses
    // the ev/od SCC ∪ cells onto ONE region exactly as at top level, and the root drops
    // at the in-lambda letrec (the binding scope). The body `(ev k)` is a tail call to
    // an SCC member — the shape whose stranded binding-scope drop rides the tail-call
    // deferred release — and must be ADMITTED (the tail-strand refusal bites only a non-member
    // callee, `merge_refuses_in_lambda_cycle_with_foreign_tail_callee`).
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(
        "(def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (ev k)))) \
         (f 3)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let (closures, _) = letrec_cycle_members(&hir, &info);
    // The cells keyed at the IN-LAMBDA letrec node (the file letrec's own cells,
    // e.g. `f`'s, live under a different begin_cell_regions key).
    let letrec_id = letrec_binding_node(&hir, &arena, &symbols, "ev")
        .expect("the in-lambda letrec binding `ev`");
    let cells: Vec<Region> = info
        .begin_cell_regions
        .get(&letrec_id)
        .map(|v| v.iter().map(|&(_, r)| r).collect())
        .unwrap_or_default();
    assert_eq!(
        cells.len(),
        2,
        "the in-lambda letrec's two forward cells are compiled static-slot cells \
         keyed at the letrec node (begin_cell_regions); got {cells:?}",
    );
    // Both cells collapse onto ONE root, and that root is one of the SCC closures.
    let cell_roots: rustc_hash::FxHashSet<Region> =
        cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        cell_roots.len(),
        1,
        "both cells must share one merged root; got {cell_roots:?} \
         merged_parent={:?}",
        info.merged_parent,
    );
    let root = cell_roots.into_iter().next().unwrap();
    assert!(
        !cells.contains(&root) && closures.contains(&root),
        "the merged root r{} must be an SCC closure region, not a cell; \
         closures={closures:?} cells={cells:?}",
        root.0,
    );
    // Exactly the two SCC closures (ev, od) join the root; the enclosing lambda
    // `f`'s own closure region stays unmerged.
    let merged_closures = closures
        .iter()
        .filter(|&&c| info.merged_root(c) == root)
        .count();
    assert_eq!(
        merged_closures, 2,
        "exactly the ev/od closures collapse onto the root (f stays unmerged); \
         closures={closures:?} merged_parent={:?}",
        info.merged_parent,
    );
    // The single drop fires at the in-lambda letrec node — the binding scope.
    let dp = info.region_data.get(&root).map(|d| d.decref_point);
    assert_eq!(
        dp,
        Some(letrec_id),
        "the in-lambda cycle (root r{}) must drop at its binding-scope letrec @{}; \
         decref_point was {:?}",
        root.0,
        letrec_id.0,
        dp,
    );
}

/// Analyze `source` under the REAL primitive classification, returning the arena
/// so `letrec_binding_node` can locate the cycle. A storing/copying `%`-op compiles
/// as a native funnel `Call`, so a body tail like `(%freeze …)` is a frame-replacing
/// `TailCall` — the shape the non-member body-tail release slot exists for.
fn analyze_cycle_with_effects(
    source: &str,
    symbols: &mut SymbolTable,
) -> (Hir, BindingArena, RegionInfo) {
    let meta = crate::primitives::build_primitive_meta(symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, &meta);
    let cc = pc.call_classification;
    let (hir, arena, _) = compile_fhir(source, symbols);
    let info = analyze_regions_with(&hir, &arena, cc);
    (hir, arena, info)
}

/// The forward-cell regions of the in-lambda `ev`/`od` letrec, and the merged root
/// they collapse onto (the SCC closure of least program order).
fn ev_od_cells(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
) -> Vec<Region> {
    let letrec_id =
        letrec_binding_node(hir, arena, symbols, "ev").expect("the in-lambda letrec binding `ev`");
    info.begin_cell_regions
        .get(&letrec_id)
        .map(|v| v.iter().map(|&(_, r)| r).collect())
        .unwrap_or_default()
}

#[test]
fn merge_admits_in_lambda_cycle_with_foreign_tail_callee() {
    // INVERTED from the old tail-strand refusal: a letrec body tail-calling a
    // NON-member closure `g` (a foreign fn) now MERGES. The frame-replacing
    // TailCall strands the binding-scope drop, but the non-member release channel —
    // `RegionInfo::cycle_tail_release` → `TailCall::deferred_release_slot` — is wired, so a
    // closure callee's new activation takes over the arena's release, freeing it at recursion
    // completion. The tail argument is `(ev k)`'s RESULT (a value), not a member, so
    // no member flows in by-move (contrast
    // `merge_refuses_member_passed_by_move_to_foreign_tail`). `g` is a user closure,
    // so its `(g r)` tail is an ordinary `Call`.
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(
        "(def g (fn [x] x)) \
         (def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (g (ev k))))) \
         (f 3)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    let roots: rustc_hash::FxHashSet<Region> = cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        roots.len(),
        1,
        "a foreign-closure body tail ((g (ev k))) must now MERGE the cycle — the \
         non-member tail release slot supplies the stranded release; cells={cells:?} \
         merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    assert!(
        !cells.contains(&root),
        "the merged root must be a closure region, not a cell; root=r{} cells={cells:?}",
        root.0,
    );
    // The non-member tail site is recorded, keyed to the merged root — the datum the
    // lowerer reads to set `deferred_release_slot`.
    assert!(
        info.cycle_tail_release.values().any(|&r| r == root),
        "the (g r) tail site must record cycle_tail_release → merged root r{}; got {:?}",
        root.0,
        info.cycle_tail_release,
    );
}

#[test]
fn merge_admits_native_tail() {
    // The native body tail `(%freeze (ev k))`: a copying `%`-op compiles as a native
    // funnel `Call`, so in tail position it is a frame-replacing `TailCall` (an inline
    // arith `%`-op would be an `Intrinsic` node and not a Call tail at all). The cycle
    // must MERGE and record the `%freeze` site in `cycle_tail_release`: at runtime the
    // native keeps the frame and the live scope-exit drop frees the arena, but the
    // release slot is carried anyway (the compiler never classifies the callee), so a
    // rebound `%freeze` closure is also covered. This is the native-tail shape the
    // whole class regressed on.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (%freeze (ev k))))) \
         (f 3)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    let roots: rustc_hash::FxHashSet<Region> = cells.iter().map(|&c| info.merged_root(c)).collect();
    assert_eq!(
        roots.len(),
        1,
        "a native body tail ((%freeze (ev k))) must MERGE the cycle; \
         cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    assert!(
        info.cycle_tail_release.values().any(|&r| r == root),
        "the (%freeze …) tail site must record cycle_tail_release → merged root r{}; got {:?}",
        root.0,
        info.cycle_tail_release,
    );
}

#[test]
fn merge_refuses_member_passed_by_move_to_foreign_tail() {
    // THE SAFETY BOUNDARY. A member closure `od` passed BY-MOVE as an argument to a
    // non-member tail call `(g od)` must REFUSE the merge. Freeing the arena at the
    // recursion's completion (the deferred release) collides with `od`'s own move/return
    // machinery — which also decrefs the merged arena — a double-free. The escape
    // gate does NOT catch this (an opaque callee's argument is not a return/fiber
    // Shared-seed), and the ANF hoist temp aliasing `od` is a synthetic holder
    // excluded from the sole-held count, so this by-move refusal is the tail gate's
    // own: `arg_bindings` sees a binding whose source region is in the SCC. Contrast
    // `merge_admits_in_lambda_cycle_with_foreign_tail_callee`, where the argument is a
    // value (`(ev k)`'s result), not the member itself.
    // `od` is used in value position (`(g od)`), so call-site forwarding cannot
    // prove its `m` — the diverging guard does (ev stays callee-only and is
    // proven by forwarding from od's `(ev (%sub m 1))`).
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(
        "(def g (fn [x] x)) \
         (def f (fn [k] (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1)))) \
                                 od (fn [m] (when (%not (%int? m)) (error :m)) \
                                      (if (%lt m 1) :odd (ev (%sub m 1))))] \
                          (g od)))) \
         (f 3)",
        &mut symbols,
    );
    let info = analyze_regions(&hir, &arena);
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    for &c in &cells {
        assert_eq!(
            info.merged_root(c),
            c,
            "a cycle passing a member (od) BY-MOVE into a non-member tail (g od) must \
             NOT merge — the deferred release would double-free the arena against od's own \
             move/return release; cell r{} merged; merged_parent={:?}",
            c.0,
            info.merged_parent,
        );
    }
    assert!(
        info.cycle_tail_release.is_empty(),
        "a refused cycle records no non-member tail release site; got {:?}",
        info.cycle_tail_release,
    );
}

#[test]
fn merge_refuses_escaping_letrec_closure() {
    // The letrec closure is the program's tail → escapes via return. An escaping
    // closure outlives the activation, so the merge must refuse it (collapsing then
    // freeing at the enclosing scope would reclaim it while the caller holds it) —
    // it stays Shared (the always-legal baseline; reclaiming an escaping closure
    // cycle awaits the owner = activation/fiber cut). `loop` is used in value
    // position (returned), so call-site forwarding cannot prove `n` — the
    // diverging guard does.
    let (hir, _, info) = pipeline(
        "(letrec [loop (fn [n] (when (%not (%int? n)) (error :n)) \
                         (if (%lt n 1) :done (loop (%sub n 1))))] loop)",
    );
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    assert_eq!(closures.len(), 1, "one closure; got {closures:?}");
    for &r in closures.iter().chain(cells.iter()) {
        assert!(
            !info.merged_parent.contains_key(&r) && !info.merged_parent.values().any(|&p| p == r),
            "an escaping (returned) letrec closure/cell r{} must not be merged; \
             merged_parent={:?}",
            r.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_collapses_self_and_sibling_captured_member_cell() {
    // A member that is BOTH self-recursive AND captured by an acyclic sibling keeps a
    // forward cell — and the single-closure self-edge admission still collapses it. `a`
    // references itself (a self-edge, resolved by the executing closure — cell-free for
    // the self-reference), while sibling `b` captures `a` (b calls a; a does NOT call b,
    // so there is no mutual cycle). The sibling capture marks `a` captured
    // (`hir/analyze/scopes.rs`), so `a` keeps its forward cell for `b`'s benefit — unlike
    // pure self-recursion, which is cell-free (`self_recursive_letrec_is_cell_free_not_merged`).
    // In the merge's capture graph `a` is a size-1 SCC with a self-edge, so the self-edge
    // admission (`collect_closure_capture_edges` keeping `r == closure_r`) is what admits
    // it: the merge collapses `a`'s forward cell into `a`'s closure region, one arena. This
    // is the case that keeps that admission LIVE post-cell-free-self-recursion — a pure
    // self-recursive closure has no cell and never reaches the merge, but this mixed member
    // does. `b` is not in any cycle, so it is not a merge member.
    let (hir, _, info) = pipeline(
        "(begin (letrec [a (fn [n] (if (%lt n 1) :done (a (%sub n 1)))) \
                         b (fn [n] (a n))] \
                  (b 3)) \
               nil)",
    );
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    assert_eq!(closures.len(), 2, "two closures (a, b); got {closures:?}");
    assert_eq!(
        cells.len(),
        1,
        "exactly one forward cell — `a`'s, kept because sibling `b` captures it; `b` is \
         captured by nothing and is cell-free. got {cells:?}",
    );
    let cell = cells[0];
    let root = info.merged_root(cell);
    assert_ne!(
        root, cell,
        "the sibling-captured self-recursive member's forward cell r{} must merge (the \
         single-closure self-edge admission collapses it into the closure region); \
         merged_parent={:?}",
        cell.0, info.merged_parent,
    );
    assert!(
        closures.contains(&root),
        "the cell r{} must collapse onto a CLOSURE region (its self-recursive owner `a`), \
         not another cell; merged_root=r{} closures={closures:?} merged_parent={:?}",
        cell.0,
        root.0,
        info.merged_parent,
    );
}

#[test]
fn merge_self_edge_refuses_clique() {
    // `has?` is declared `Mixed` (a trait-dispatched native), so it keeps the full
    // may-store clique between its two heap (string-literal) args. A clique edge is not a
    // `%pair` immutable store, so the merge seed never touches it — its endpoints keep
    // distinct merge roots and the predicate must refuse it. Its balancing decref is the
    // target's runtime content scan; eliminating it trades a known leak for a possible
    // UAF. Uses the REAL classification (`has?` genuinely Mixed), not a forced effect.
    let (hir, arena, symbols, info) = analyze_with_class("(has? \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "has?", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (has? ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        !edges.is_empty(),
        "precondition: the Mixed native keeps its arg clique"
    );
    for (src, dst) in edges {
        assert!(
            !info.is_merge_self_edge(src, dst),
            "a may-store clique edge (src r{}, dst r{}) must NOT be flagged a \
             self-edge; merged_parent={:?}",
            src.0,
            dst.0,
            info.merged_parent,
        );
    }
}
