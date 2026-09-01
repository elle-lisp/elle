use super::*;

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
    let (hir, arena) = compile_fhir(
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
    let (hir, arena) = compile_fhir(
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
    let (hir, arena) = compile_fhir(
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
    let (hir, arena) = compile_fhir(
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
fn merge_refuses_returned_cell_free_self_recursive_closure() {
    // A RETURNED purely self-recursive letrec closure is not the merge's business at
    // all, and the gate that refuses it is CELL-FREEDOM, not the frontier: the
    // self-edge does not mark `loop` captured (`hir/analyze/scopes.rs`), so it mints no
    // forward cell, and `cell_of` has nothing to pair in. Its release is the cell-free
    // stranded-self deferral instead (docs/impl/selfrec.md), which admits the return
    // facet on its own argument — so this refusal must NOT be read as "returned ⇒
    // refused": a returned MUTUAL cycle, which does have cells, merges
    // (`merge_admits_returned_member_cycle_on_member_tail`). `loop` is used in value
    // position (returned), so call-site forwarding cannot prove `n` — the diverging
    // guard does.
    let (hir, _, info) = pipeline(
        "(letrec [loop (fn [n] (when (%not (%int? n)) (error :n)) \
                         (if (%lt n 1) :done (loop (%sub n 1))))] loop)",
    );
    let (closures, cells) = letrec_cycle_members(&hir, &info);
    assert_eq!(closures.len(), 1, "one closure; got {closures:?}");
    assert!(
        cells.is_empty(),
        "the gate under test: a purely self-recursive `loop` mints NO forward cell, \
         so the merge has no cell to pair in; got cells {cells:?}",
    );
    for &r in closures.iter().chain(cells.iter()) {
        assert!(
            !info.merged_parent.contains_key(&r) && !info.merged_parent.values().any(|&p| p == r),
            "a cell-free self-recursive letrec closure r{} must not be merged; \
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
