use super::*;

#[test]
fn merge_self_edge_refuses_clique() {
    // `fiber/resume` is declared `Mixed` (it installs its resume value into the target
    // fiber's `signal` field, uncounted at compile time), so it keeps the full may-store
    // clique between its two heap (string-literal) args. A clique edge is not a
    // `%pair` immutable store, so the merge seed never touches it — its endpoints keep
    // distinct merge roots and the predicate must refuse it. Its balancing decref is the
    // target's runtime content scan; eliminating it trades a known leak for a possible
    // UAF. Uses the REAL classification (`fiber/resume` genuinely Mixed), not a forced
    // effect.
    let (hir, arena, symbols, info) = analyze_with_class("(fiber/resume \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "fiber/resume", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (fiber/resume ...) call");
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

#[test]
fn merge_admits_returned_member_cycle_on_member_tail() {
    // THE RETURN-FUNDED ADMISSION (docs/impl/region/letrec.md § The frontier gate).
    // The ev/od SCC with a member RETURNED: `ev`'s base case hands back `ev` itself,
    // so `ev`'s region carries escape's return facet. The cycle must still MERGE.
    //
    // Why the return facet is not a reason to refuse: the merge collapses `ev`'s
    // region onto the arena, so the value handed out lives IN the arena and the
    // callee's `Return` mint raises the arena's own count. The release the merge owns
    // is the frame's, and the letrec body's tail is a call to the MEMBER `ev` — so the
    // binding-scope `DecrefRegion` is dead past that frame-replacing `TailCall` and the
    // release rides the member deferral, which `trampoline_loop` runs at the recursion's
    // NORMAL COMPLETION, after every `Return` mint on the taken path. The caller's
    // reference is standing when the frame's is dropped. This is the mutual twin of the
    // cell-free self-recursive deferral's return admission (docs/impl/selfrec.md § "The
    // deferral's escape gate is the fiber frontier alone").
    //
    // `ev` is used in value position (returned), which disables call-site param joins,
    // so the diverging guards prove the `%lt`/`%sub` operands — as in the oracle's
    // `recur-local-mutual-ret` probe, whose 4 regions / 6 objects per call this closes.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def f (fn [k] \
           (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (od (%sub m 1)))) \
                    od (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (ev (%sub m 1))))] \
             (ev k)))) \
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
        "a RETURNED member's cycle must merge onto ONE arena — the member-callee tail \
         deferral runs after the Return mint; cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
}

#[test]
fn merge_admits_returned_cycle_on_non_member_tail() {
    // THE RETURN-FUNDED ADMISSION, non-member-tail face. The same returned-member
    // cycle, but the letrec body tail-calls a NON-member `g`. Which channel carries the
    // arena's release does not enter the ordering argument, and neither does the fact
    // that the compiler cannot classify `g`: BOTH of the callee's runtime resolutions
    // release after the mint. A closure `g` replaces the frame, so the binding-scope
    // `DecrefRegion` is dead and the release rides `deferred_release_slot`, which
    // `trampoline_loop` runs at the recursion's normal completion — after `g`'s own
    // `Return` mint. A native `g` keeps the frame and falls through to that same
    // binding-scope drop, which the lowerer emits at the `Letrec` node, i.e. AFTER the
    // whole body — and so after the mint the tail call itself emits at the call site
    // (the post-`TailCall` fall-through retain).
    //
    // What the admission needs is therefore only that the BODY hands the value over
    // itself, which a tail call does (docs/impl/region/letrec.md § The frontier gate).
    // This is the oracle probe `recur-local-mutual-ret-foreign`, whose 4 regions /
    // 6 objects per call it closes.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def g (fn [x] x)) \
         (def f (fn [k] \
           (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (od (%sub m 1)))) \
                    od (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (ev (%sub m 1))))] \
             (g (ev k))))) \
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
        "a returned cycle whose body tail-calls a NON-member must merge onto ONE arena \
         — both of that callee's resolutions release after the mint; cells={cells:?} \
         merged_parent={:?}",
        info.merged_parent,
    );
    // The non-member tail keeps its own channel: the closure resolution needs the
    // explicit slot, since no member deferral covers it.
    assert!(
        !info.cycle_tail_release.is_empty(),
        "the non-member tail must be recorded as an arena adopt site, or a closure \
         callee's frame replacement strands the binding-scope drop; \
         cycle_tail_release={:?}",
        info.cycle_tail_release,
    );
}

#[test]
fn merge_admits_returned_cycle_on_value_tail() {
    // THE RETURN-FUNDED ADMISSION, value-tail face. The letrec body's tail is the bare
    // member value `ev`, so there is no tail call at all — but the letrec IS `f`'s tail,
    // so functionalization puts `f`'s `Return` INSIDE the letrec body. `lower_return`
    // mints there, and the binding-scope `DecrefRegion` is emitted at the `Letrec` node
    // after the body's whole lowering, so the release still follows the mint. The body
    // hands the value over itself, which is all the admission asks.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def f (fn [k] \
           (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (od (%sub m 1)))) \
                    od (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) ev (ev (%sub m 1))))] \
             ev))) \
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
        "a returned cycle whose letrec is its frame's tail must merge onto ONE arena — \
         the `Return` inside the body mints before the binding-scope drop; \
         cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
}

#[test]
fn merge_admits_returned_cycle_bound_out_of_tail_position() {
    // THE HANDED-OUT MEMBER (docs/impl/region/letrec.md § "Drop site — following a
    // handed-out member"). The identical returned cycle, one binding out of tail
    // position: the letrec's value is bound to `c` and handed on by a LATER statement,
    // so the body falls out to a bare value with no `Return` and no tail call of its
    // own. `c` names `ev`'s region directly — a `DerefCell` read of the member mints
    // nothing — so the member outlives the binding scope on the cycle's own reference,
    // and a release pinned at the letrec would take the arena to zero under a live
    // holder.
    //
    // That is a reason to move the release, not to refuse the cycle. `ev`'s region
    // already carries a `decref_point`: the last-use rule extended it through `c` and
    // pinned it at the enclosing `Return`, whose mint runs before that node's own
    // `emit_decrefs_for`. The merge adopts that point as the arena's drop site — the
    // release the compiler already emits for this member, now covering the siblings and
    // cells whose uses it post-dominates — and waives the sole-held proxy for the member
    // it followed.
    //
    // Both halves are asserted, because either alone is unsound: merging while the drop
    // stays at the letrec frees the arena under `c`, and moving the drop without merging
    // reclaims nothing. This is the boundary control for
    // `merge_admits_returned_cycle_on_value_tail` — the two differ only in whether the
    // letrec is the frame's tail, which decides whether the mint is inside the body or
    // an enclosing node away.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def g (fn [x] x)) \
         (def f (fn [k] \
           (let [c (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (od (%sub m 1)))) \
                            od (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (ev (%sub m 1))))] \
                     ev)] \
             (g 1) \
             c))) \
         (f 3)",
        &mut symbols,
    );
    let letrec_id =
        letrec_binding_node(&hir, &arena, &symbols, "ev").expect("the in-lambda letrec binding");
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
        "a returned cycle bound OUT of its frame's tail must still merge onto ONE arena \
         — the handed-out member's own release point is late enough to fund it; \
         cells={cells:?} merged_parent={:?}",
        info.merged_parent,
    );
    let root = roots.into_iter().next().unwrap();
    let drop_site = info
        .region_data
        .get(&root)
        .expect("the merged root carries the arena's single release")
        .decref_point;
    // The release must have FOLLOWED THE VALUE OUT. Structurally, not numerically: the
    // drop site must post-dominate the binding scope while lying outside its subtree —
    // exactly what pinning it at the letrec (`drop_site == letrec_id`) fails to do.
    let order = compute_order(&hir);
    let pd = super::super::postdom::PostDom::new(&hir, &order);
    assert!(
        !pd.in_subtree(drop_site, letrec_id),
        "the arena's release must follow the handed-out member OUT of the binding scope, \
         but the drop site @{} still lies inside the letrec @{}'s subtree — `c` reads the \
         member past a `DecrefRegion` that already took the arena to zero",
        drop_site.0,
        letrec_id.0,
    );
    assert!(
        pd.drop_post_dominates(drop_site, letrec_id, super::super::postdom::EmitMode::Merge),
        "the adopted drop site @{} must post-dominate the binding scope @{} — every \
         member's binding-scoped use is inside it",
        drop_site.0,
        letrec_id.0,
    );
}

#[test]
fn merge_refuses_fiber_crossing_handed_out_member() {
    // The fiber half of the frontier gate over the HANDED-OUT shape. Following a member
    // out of the binding scope waives the sole-held proxy for it, so the binding it is
    // bound out to no longer refuses the cycle by itself — which makes this the pin that
    // the FIBER facet is still read on that path. `c` is yielded, so the member reaches a
    // resumer whose hold the compiler did not place and a parked frame may borrow it
    // uncounted; no release point, however late, funds that.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(fiber/new (fn () \
           (let [c (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (od (%sub m 1)))) \
                            od (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (ev (%sub m 1))))] \
                     ev)] \
             (begin (yield c) (c 3)))) \
           |:yield|)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    for &c in &cells {
        assert!(
            !info.merged_parent.contains_key(&c) && !info.merged_parent.values().any(|&p| p == c),
            "a cycle whose handed-out member is YIELDED must NOT merge — the fiber \
             frontier is refused outright, and following the value out does not change \
             that; cell r{} merged_parent={:?}",
            c.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_refuses_returned_cycle_whose_body_hands_out_nothing() {
    // THE RESIDUAL of the two orderings. The same returned cycle out of tail position,
    // but the body's tail is a non-tail `(g ev)` rather than the bare member: it does
    // not leave the frame (so no mint stands inside the body), and what the letrec
    // yields is the CALL's result — a fresh region, not a member — so there is no
    // handed-out member whose release point the arena could follow either. `ev` is on
    // the return frontier (its base case hands itself back) with nothing to order that
    // against, so the cycle keeps the Shared baseline.
    //
    // This is the counterfactual that keeps the two admissions honest: neither may be
    // read as "a returned cycle out of tail position merges". The one that does
    // (`merge_admits_returned_cycle_bound_out_of_tail_position`) differs only in the
    // body's tail naming the member itself.
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(def g (fn [x] x)) \
         (def f (fn [k] \
           (let [c (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (od (%sub m 1)))) \
                            od (fn [m] (when (%not (%int? m)) (error :m)) \
                                 (if (%lt m 1) ev (ev (%sub m 1))))] \
                     (g ev))] \
             (g 1) \
             c))) \
         (f 3)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    for &c in &cells {
        assert!(
            !info.merged_parent.contains_key(&c) && !info.merged_parent.values().any(|&p| p == c),
            "a returned cycle whose body neither leaves the frame nor hands a member \
             out must NOT merge — no mint is inside the body and no member's release \
             point is available to follow; cell r{} merged_parent={:?}",
            c.0,
            info.merged_parent,
        );
    }
}

#[test]
fn merge_refuses_fiber_crossing_letrec_cycle() {
    // THE FIBER HALF of the frontier gate still refuses, and it is the half that
    // cannot be return-funded: a YIELDED member crosses to a resumer whose hold the
    // compiler did not place, and a parked frame may borrow it uncounted — no mint
    // funds that, so no ordering argument admits it. The letrec body's tail is the
    // member call `(ev 3)`, so the TAIL shape passes and the fiber facet is the only
    // gate left to bite (making this the live pin for that half).
    let mut symbols = SymbolTable::new();
    let (hir, arena, info) = analyze_cycle_with_effects(
        "(fiber/new (fn () \
           (letrec [ev (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) :even (od (%sub m 1)))) \
                    od (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) :odd (ev (%sub m 1))))] \
             (begin (yield ev) (ev 3)))) \
           |:yield|)",
        &mut symbols,
    );
    let cells = ev_od_cells(&hir, &arena, &symbols, &info);
    assert_eq!(
        cells.len(),
        2,
        "precondition: two compiled forward cells; got {cells:?}"
    );
    for &c in &cells {
        assert!(
            !info.merged_parent.contains_key(&c) && !info.merged_parent.values().any(|&p| p == c),
            "a cycle with a YIELDED member must NOT merge — the fiber frontier is \
             refused outright; cell r{} merged_parent={:?}",
            c.0,
            info.merged_parent,
        );
    }
}
