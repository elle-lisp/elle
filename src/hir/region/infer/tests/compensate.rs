use super::*;

// ── The return frontier is per-path ───────────────────────────────────
//
// A returned region is the caller's to free only on the paths that hand it over.
// On a sibling arm that never uses the value no return mint fires, the caller
// receives nothing, and the callee still holds the only reference — so that arm
// owes a compensating release even though escape marks the region returnable.
// See docs/impl/region/mechanism.md § "The return frontier is per-path" and the
// end-to-end pin tests/elle/region-return-arm-escape-leak.lisp.

/// The body HirIds of the first `Match` in the tree, in arm order.
fn first_match_arms(hir: &Hir) -> Option<Vec<HirId>> {
    if let HirKind::Match { arms, .. } = &hir.kind {
        return Some(arms.iter().map(|(_p, _g, body)| body.id).collect());
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_match_arms(c);
        }
    });
    found
}

/// Does `arm` carry a compensating release for any region the binding named
/// `name` may point into?
fn arm_compensates(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    arm: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_compensation
        .get(&arm)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// Does every region the binding named `name` may point into carry its release
/// OUTSIDE `arms` — i.e. at a node every arm reaches?
///
/// This is the branch-arm release window's signature (docs/impl/region/
/// mechanism.md § "A release inside one arm is not a release on the other
/// arms"): the region's one `decref_point` is re-anchored onto the branch, so no
/// arm holds it and none needs a compensating one.
fn release_clears_the_arms(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    arms: &[HirId],
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    regions
        .iter()
        .all(|&r| region_release_clears_the_arms(hir, info, r, arms))
}

/// Every binder of `name` that records a release route, as `(binding, region)` —
/// the region its `Let`/`Letrec`/`Define` INIT allocated, which is what
/// `region_to_slot` is keyed on (docs/impl/region/mechanism.md § "A release the
/// relocation replicates names a VALUE, and a binder's slot supplies that name").
/// More than one entry where functionalization split the name into versions, each
/// with an allocating init of its own.
fn binder_routes(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
) -> Vec<(Binding, Region)> {
    fn walk(
        h: &Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
        info: &RegionInfo,
        out: &mut Vec<(Binding, Region)>,
    ) {
        let mut record = |b: &Binding, init: &Hir| {
            if symbols.name(arena.get(*b).name) == Some(name) {
                out.extend(info.alloc_region.get(&init.id).map(|&r| (*b, r)));
            }
        };
        match &h.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, init) in bindings {
                    record(b, init);
                }
            }
            HirKind::Define { binding, value, .. } => record(binding, value),
            _ => {}
        }
        h.for_each_child(|c| walk(c, name, arena, symbols, info, out));
    }
    let mut out = Vec::new();
    walk(hir, name, arena, symbols, info, &mut out);
    assert!(!out.is_empty(), "`{name}` has no allocating binder");
    out
}

/// [`release_clears_the_arms`] for ONE region, so a pin can name the region it
/// means rather than every region its holder may point into — an env-celled
/// binding holds a box placeholder beside the value's own region, and the two
/// answer to different release routes.
fn region_release_clears_the_arms(hir: &Hir, info: &RegionInfo, r: Region, arms: &[HirId]) -> bool {
    let order = compute_order(hir);
    let low = compute_subtree_low(hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    match info.region_data.get(&r) {
        Some(d) => {
            let o = ord(d.decref_point);
            !arms
                .iter()
                .any(|&a| low.get(&a).copied().unwrap_or(0) <= o && o <= ord(a))
        }
        None => false,
    }
}

// ── The obligation, and the two routes that discharge it ─────────────
//
// No path may leave a branch without releasing a region that was live-in to it.
// Two mechanisms discharge that one obligation, and the routing is a property of
// the region and the branch together. The window moves the region's single
// release to a point every arm reaches — admitted only where escape proves the
// frame holds the region alone. Where an arm leaves through a frame-replacing
// callee it reaches no merge, and the frame-exit relocation covers it instead, so
// such a branch narrows the window to the value-routed releases that relocation
// can replicate. Everything else keeps the in-arm release plus the per-arm
// compensation routes, which carry a count argument instead. The tests below pin
// each route on the shape that selects it.

#[test]
fn a_returned_param_anchors_where_no_arm_leaves_the_frame() {
    // `(if (%eq i 0) xs 7)` — `xs` leaves through the THEN arm, so its
    // `decref_point` lands there and the ELSE arm hands the caller an immediate.
    // Both arms arrive at the merge, and the merge owes the return facet no
    // funding edge: the returning arm ran its mint before jumping here, and the
    // other handed nothing over (docs/impl/region/mechanism.md § "The return facet
    // costs the merge nothing"). So the one release moves to the branch and neither
    // compensation route fires.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a returned param's release must sit where both arms reach it"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_returned_param_anchors_whichever_arm_carries_it_out() {
    // The mirror: `xs` leaves through the ELSE arm. Pins that the admission reads
    // arm structure and not arm position.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) 7 xs))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a returned param's release must sit where both arms reach it"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_frame_exit_the_callee_cannot_reach_anchors_a_returned_param() {
    // The ELSE arm leaves through a callee that neither names `xs` nor captures it.
    // A callee reaches a value this frame owns as an operand or through its captured
    // environment and by no other route, so this one cannot name `xs` at all — its
    // `Return` mints nothing against that region and the replica ahead of the
    // `TailCall` is the region's last release (docs/impl/region/mechanism.md § "The
    // callee's return mint, and why the point owes it nothing"). So the branch
    // anchors the return facet here exactly as it does where the callee captures.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs (g 7)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a frame exit the callee cannot reach must not decline the returned param"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id)
            && !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn an_index_walk_fold_driver_anchors_its_accumulator() {
    // The everyday shape the admission above reaches: the base arm returns the
    // accumulator, and the recursive arm hands the tail callee the COMBINER's result
    // rather than `acc` itself. No route reaches `acc` at that point, so `acc`'s one
    // release anchors on the branch and each displaced accumulator is freed per step
    // (`fold`/`reduce`/`concat` are the callers).
    let (hir, arena, symbols, info) = analyze_with_class(
        "(begin (def step (fn (f n i acc) \
           (if (%lt i n) (step f n (%add i 1) (f acc i)) acc))) (step g 2 0 nil))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "acc", &[then_id, else_id]),
        "the fold driver's accumulator must be released where both arms reach it"
    );
}

#[test]
fn read_only_arm_release_clears_the_arms() {
    // Control: when no arm carries `xs` across the return frontier the same
    // anchoring applies. Guards against a change that treats the returned shape
    // specially by dropping the baseline.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) 7))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a merely-read param's release must sit where both arms reach it"
    );
}

#[test]
fn match_arms_are_treated_like_if_arms() {
    // Every premise here is stated over ONE ARM and its siblings — never over the
    // branch's arity or kind. `v` is allocated before the dispatch, so it is
    // live-in on every arm and no arm may hold its only release.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert_eq!(arms.len(), 3, "the dispatch has three arms");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "v", &arms),
        "a Match's live-in local must be released where every arm reaches it"
    );
}

#[test]
fn a_frame_replacing_arm_anchors_a_value_routed_release() {
    // `(g 7)` in tail position replaces the frame, so the ELSE arm leaves through
    // the callee rather than arriving at the merge. The anchor alone does not
    // cover that arm — the frame-exit relocation replicates the anchored release
    // ahead of its `TailCall` — so the branch narrows to the releases that
    // relocation can replicate instead of declining whole
    // (docs/impl/region/mechanism.md § "An arm that leaves through a callee takes
    // a replica, not the anchor"). Only a VALUE route is replicable, and a call
    // result is value-routed unconditionally — which is what the first assertion
    // states about this shape and the second relies on.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (g 7)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let b = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the param `xs`");
    assert!(
        info.binding_source_regions.get(&b).is_some_and(
            |rs| !rs.is_empty() && rs.iter().all(|r| info.call_result_regions.contains(r))
        ),
        "the narrowing admits only value-routed regions, so this pin needs `xs` \
         to be one — a region released by id keeps the whole-branch decline"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a frame-replacing sibling arm must not decline a value-routed release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_frame_replacing_arm_anchors_a_binder_routed_release() {
    // The same branch shape, over a region the lowerer releases by ID unless some
    // point admits it: `xs` is a `%pair` allocation the `let` binder owns, so it
    // is no call result. `record_region_slot` still keyed a slot on that
    // allocation, so the relocation can take the value route and replicate the
    // release into the frame-replacing arm (docs/impl/region/mechanism.md § "A
    // release the relocation replicates names a VALUE, and a binder's slot supplies
    // that name"). The window asks that question rather than reading the region's
    // class, so this branch narrows to `xs` instead of declining it.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i) (let [xs (%pair 1 nil)] (if (%eq i 0) (length xs) (g 7))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let b = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the binding `xs`");
    assert!(
        info.binding_source_regions.get(&b).is_some_and(
            |rs| !rs.is_empty() && rs.iter().all(|r| !info.call_result_regions.contains(r))
        ),
        "the discriminator: this pin is about a region OUTSIDE `call_result_regions`, \
         which is what the class reading declined"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a frame-replacing sibling arm must not decline a binder-routed release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id)
            && !arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_binders_allocation_is_value_routed() {
    // The positive half of the mirror, stated where the window reads it: an
    // ordinary `let` binder's slot holds its init's value from the binder to the
    // release, so the region that init allocated can be released by value and
    // replicated into an arm.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i) (let [xs (%pair 1 nil)] (if (%eq i 0) (length xs) (g 7))))");
    let routes = binder_routes(&hir, &arena, &symbols, &info, "xs");
    assert!(
        routes
            .iter()
            .all(|(_, r)| info.value_routed_regions.contains(r)),
        "{routes:?} is what an ordinary binder's slot names; value_routed={:?}",
        info.value_routed_regions
    );
}

#[test]
fn a_celled_binders_allocation_is_not_value_routed() {
    // The refusal the binder half keeps. `xs` is captured, so its binder stored the
    // value into an env cell rather than into a stack slot naming the value — the
    // slot a release would load holds the BOX. With no value route the frame-exit
    // relocation can replicate nothing, so the branch-arm window must keep the
    // whole-branch decline rather than anchor a release the exiting arm never runs.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (i) (let [@xs (%pair 1 nil) f (fn () (length xs))] \
           (if (%eq i 0) (%add (length xs) (f)) (g 7))))",
    );
    let routes = binder_routes(&hir, &arena, &symbols, &info, "xs");
    assert!(
        routes
            .iter()
            .all(|(_, r)| !info.value_routed_regions.contains(r)),
        "{routes:?} is stored into an env cell, so no slot names its value; \
         value_routed={:?}",
        info.value_routed_regions
    );
}

#[test]
fn a_callee_the_arm_tail_calls_keeps_its_in_arm_release() {
    // The boundary the value-route reading must stop at. `go`'s own closure region
    // is what the exiting arm's call names as its CALLEE, so the frame-exit
    // relocation exempts it and replicates nothing into that arm — the deferred
    // callee channel runs that release from where it sits instead
    // (docs/impl/region/mechanism.md § "What the exemption keeps, a channel must
    // still run"). Anchoring it at the merge would take it out of that channel's
    // reach and leave the arm with no release at all, so the branch declines it.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (xs) (letrec [go (fn (a b) a)] (if (%eq xs 0) 0 (go xs 1))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let routes = binder_routes(&hir, &arena, &symbols, &info, "go");
    assert!(
        routes
            .iter()
            .all(|(_, r)| info.value_routed_regions.contains(r)),
        "the discriminator: {routes:?} is binder-routed, so only the callee \
         exemption can decline it"
    );
    assert!(
        routes.iter().all(|(_, r)| !region_release_clears_the_arms(
            &hir,
            &info,
            *r,
            &[then_id, else_id]
        )),
        "a tail callee's own closure region must keep its in-arm release; got \
         {routes:?}"
    );
}

#[test]
fn a_reassigned_binder_versions_away_before_it_can_route() {
    // Why the mirror's reassign refusal is a backstop rather than a shape:
    // functionalization gives an in-function reassignment one version per store, so
    // the binder that ALLOCATES is not the binding an `assign` repoints — a loop
    // carries the value through a `Loop` parameter, which allocates nothing and so
    // records no route. The refusal keeps the emitter's own
    // `reassigned_local_slots` reading honest where that does not hold; here it
    // costs nothing, and the route stays available.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (@i n) (let [@xs (%pair 1 nil)] \
           (begin (while (%lt i n) (begin (assign xs (%pair i xs)) (assign i (%add i 1)))) \
             (if (%eq i 0) (length xs) (g 7)))))",
    );
    let routes = binder_routes(&hir, &arena, &symbols, &info, "xs");
    assert!(
        routes
            .iter()
            .all(|(b, _)| !info.reassigned_local_bindings.contains(b)),
        "the allocating binder is a version no `assign` repoints; got {routes:?}"
    );
}

#[test]
fn a_capturing_frame_exit_anchors_a_returned_param() {
    // The `push-all` shape: the THEN arm returns `dst`, and the ELSE arm leaves
    // through a local walker that reaches `dst` only through its captured
    // environment. That capture is the funnel's counted edge, so it holds the
    // region off zero until the walker's own `Return` mints the caller's reference
    // — the other end of the enumeration the shape above drives, and the branch
    // anchors the return facet on both.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (dst n) (if (%eq n 0) dst \
           (letrec [go (fn (i) (if (%lt i n) (go (%add i 1)) dst))] (go 0))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "dst", &[then_id, else_id]),
        "a capture-funded frame exit must not decline the returned param"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "dst", then_id)
            && !arm_compensates(&hir, &arena, &symbols, &info, "dst", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_native_tail_arm_does_not_decline_the_window() {
    // A tail call to a NATIVE pushes no frame and falls through to the merge, so
    // it is not a frame exit at all and the narrowing above never applies — the
    // distinction is the callee kind, not `is_tail`.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (length xs)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a native tail call in an arm must not decline the window"
    );
}

// ── An arm is a conditional position, not a syntactic arm body ───────
//
// A `cond`'s clause TESTS are conditional positions exactly as its bodies are,
// and an `and`/`or` tail is one with no body at all. Reading those forms
// syntactically leaves a release whose last use is a later test outside every
// arm — which is where the polymorphic entry point puts it (see
// docs/impl/region/mechanism.md § "An arm is a conditional position, not a
// syntactic arm body" and the end-to-end rows in
// tests/elle/region-branch-arm-window.lisp).

/// Every child of the first `Cond` in the tree — each clause's test and body, and
/// the `else` branch. The window's signature for a `cond` is that the release
/// clears all of them: the anchor is the form's own consuming node, which no
/// child's subtree contains.
fn first_cond_parts(hir: &Hir) -> Option<Vec<HirId>> {
    if let HirKind::Cond {
        clauses,
        else_branch,
    } = &hir.kind
    {
        let mut out: Vec<HirId> = Vec::new();
        for (test, body) in clauses {
            out.push(test.id);
            out.push(body.id);
        }
        out.extend(else_branch.iter().map(|e| e.id));
        return Some(out);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_cond_parts(c);
        }
    });
    found
}

/// Every element of the first `And`/`Or` in the tree.
fn first_short_circuit_parts(hir: &Hir) -> Option<Vec<HirId>> {
    if let HirKind::And(exprs) | HirKind::Or(exprs) = &hir.kind {
        return Some(exprs.iter().map(|e| e.id).collect());
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_short_circuit_parts(c);
        }
    });
    found
}

#[test]
fn a_cond_clause_test_is_a_conditional_position() {
    // `xs`'s last use is the SECOND clause's test, which runs only where the first
    // clause's test failed. Every call that takes the first body skips it, so a
    // release left there fires on no such path at all. The arms of a `cond` are its
    // nested-`If` equivalent — the clause body, and the rest of the chain from the
    // next test on — so the one release anchors on the form's own merge.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (cond (%eq t 0) 1 (%lt 0 (length xs)) 2 true 0))");
    let parts = first_cond_parts(&hir).expect("a Cond node");
    assert_eq!(parts.len(), 6, "three clauses, each a test and a body");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &parts),
        "a live-in param whose last use is a later clause's TEST must be released \
         where every clause reaches it"
    );
}

#[test]
fn a_cond_body_is_an_arm_like_any_other() {
    // The body half of the same decomposition: `xs`'s last use is the LAST clause's
    // body, so every earlier clause strands it. `Cond` is a branch, so its bodies
    // are arms exactly as an `If`'s and a `Match`'s are.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (cond (%eq t 0) 1 (%eq t 1) (length xs) true 0))");
    let parts = first_cond_parts(&hir).expect("a Cond node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &parts),
        "a live-in param used by one clause body must be released where every \
         clause reaches it"
    );
}

#[test]
fn a_short_circuit_tail_is_an_arm() {
    // `(or a b)` evaluates `b` only where `a` is falsy, so `b` is a conditional
    // position with no sibling body — a one-armed branch. `xs`'s last use sits
    // there, and the path that short-circuits must still release it.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (if (or t (%lt 0 (length xs))) 1 2))");
    let parts = first_short_circuit_parts(&hir).expect("an Or node");
    assert_eq!(parts.len(), 2, "the `or` has two elements");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &parts[1..]),
        "a live-in param whose last use is a short-circuited tail must be released \
         where both paths reach it"
    );
}

#[test]
fn an_and_tail_is_an_arm_too() {
    // The `and` face of the same rule: the tail runs only where the head is truthy.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (if (and t (%lt 0 (length xs))) 1 2))");
    let parts = first_short_circuit_parts(&hir).expect("an And node");
    assert_eq!(parts.len(), 2, "the `and` has two elements");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &parts[1..]),
        "a live-in param whose last use is a short-circuited `and` tail must be \
         released where both paths reach it"
    );
}

#[test]
fn an_arm_whose_loop_reads_a_live_in_param_anchors_at_the_branch() {
    // The window's iterative boundary is the loop's BODY, not the loop's own node.
    // A read of a loop-external binding is anchored at the loop NODE
    // (docs/impl/region/mechanism.md § "Every binder records its scope"), and the
    // lowerer emits a node's releases after it, so that release already runs once
    // per execution of the loop — the same count with which the merge is reached.
    // Reading the boundary as the closed subtree interval would leave the branch's
    // only release under the looping arm, stranding `xs` on every other arm.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t xs) (if t (length xs) \
           (begin (def @i 0) (while (%lt i 3) (length xs) (assign i (%add i 1))) 0)))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        find_first(&hir, |h| matches!(
            &h.kind,
            HirKind::While { .. } | HirKind::Loop { .. }
        ))
        .is_some(),
        "the shape must contain an iterative scope for the boundary to be read"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a live-in param a nested loop merely READS must be released where every \
         arm reaches it"
    );
}

#[test]
fn an_alias_the_arm_introduces_does_not_defeat_the_live_in_premise() {
    // The live-in premise keeps out a value BORN inside an arm, and "born" is the
    // allocation: `record_region_slot` keys `region_to_slot` on a region's
    // allocation site, so a binding whose init merely names another one records no
    // slot and can never be the release's route
    // (docs/impl/region/mechanism.md § "The boundaries"). `w` here is such a
    // binding, so `xs` is still live-in and its one release moves to the merge.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (if t (length xs) (let [w xs] (length w))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "an alias the arm introduces must not read as a birth in the arm"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

// ── The mutated refusal is about the route, and one binding owns it ──
//
// `region_to_slot` is keyed on a region's allocation site, so the slot a
// value-routed release loads belongs to the binding whose init allocated the
// region — or, where nothing in this body allocates it, to the parameter the
// lambda prologue recorded. Every other holder names the same value through a slot
// no release reads, so the mutated question is asked of the route's binding alone
// (docs/impl/region/mechanism.md § "A mutated holder poisons its value route, not
// its cell box"). The pins below are the admissions and the refusals they keep.

#[test]
fn a_reassigned_destructured_name_refuses_nothing() {
    // The binder forms that record a route are `Define`, `Let`/`Letrec` and the
    // lambda prologue, and no others. A DESTRUCTURING name is none of them: the
    // pattern extracts it from the scrutinee, and the lowerer records no
    // `region_to_slot` entry for it, so no value-routed release can load the slot
    // its `assign` repoints. The destructured list lives in the ANF temp that
    // produced it, whose own slot is bound once and never repointed — reassigning
    // one of the names the pattern introduced must not refuse it.
    let (hir, arena, symbols, info) =
        analyze_with_class("(begin (def (@a @b) (list 1 2)) (assign a 10) (length b))");
    let mutated: Vec<Binding> = info
        .binding_source_regions
        .keys()
        .copied()
        .filter(|&b| arena.get(b).is_mutated)
        .collect();
    assert_eq!(
        mutated.len(),
        1,
        "one reassigned binding; got {:?}",
        mutated
            .iter()
            .map(|&b| symbols.name(arena.get(b).name))
            .collect::<Vec<_>>()
    );
    let a = mutated[0];
    assert_eq!(
        symbols.name(arena.get(a).name),
        Some("a"),
        "precondition: the reassigned binding is the destructured `a`"
    );
    let allocs = find_calls_to_primitive(&hir, "list", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "one `list` literal; got {allocs:?}");
    let r = *info
        .alloc_region
        .get(&allocs[0])
        .expect("the list literal allocates a region");
    assert!(
        info.binding_source_regions
            .get(&a)
            .is_some_and(|rs| rs.contains(&r)),
        "precondition: the destructured name holds the scrutinee's region, which is \
         what made the whole-holder reading refuse it"
    );
    assert!(
        info.frame_held_regions.contains(&r),
        "r{} routes through the temp that produced the list, and a destructuring \
         name records no route at all, so it must not be refused; frame_held={:?}",
        r.0,
        info.frame_held_regions,
    );
}

#[test]
fn a_reassigned_parameter_has_no_route_but_its_box() {
    // The parameter half of the same reading, and why the prologue's own set is
    // empty in practice: `needs_capture` at parameter scope IS `is_mutated`, so a
    // reassigned parameter is celled, and the one region it names is that cell's —
    // released by naming the BOX, which `populate_env` mints once per activation
    // and no `assign` repoints. So the prologue records no poisonable route, and
    // the call result the body assigns into the parameter keeps its own.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t @p) (begin (assign p (rest p)) (if t (length p) 0)))");
    let p = find_binding_by_name(&hir, "p", &arena, &symbols).expect("the param `p`");
    assert!(
        arena.get(p).is_mutated && arena.get(p).needs_capture(),
        "precondition: a reassigned parameter is celled"
    );
    let p_regions = info
        .binding_source_regions
        .get(&p)
        .cloned()
        .unwrap_or_default();
    assert!(
        !p_regions.is_empty()
            && p_regions
                .iter()
                .all(|r| info.cell_release_regions.contains(r)),
        "the celled parameter names its env cell and nothing else; p={p_regions:?}"
    );
    let calls = find_calls_to_primitive(&hir, "rest", &arena, &symbols);
    assert_eq!(calls.len(), 1, "one `rest` call; got {calls:?}");
    let r = *info
        .alloc_region
        .get(&calls[0])
        .expect("the call result has a placeholder region");
    assert!(
        info.frame_held_regions.contains(&r),
        "r{} is the assigned value's own region, not the cell's, so the parameter's \
         reassignment must not refuse it; frame_held={:?}",
        r.0,
        info.frame_held_regions,
    );
}

#[test]
fn a_cursor_an_arm_walks_does_not_refuse_the_live_in_release() {
    // The everyday `each` over a list: the type dispatch receives the cons chain,
    // and the arm that walks it opens by binding a reassigned cursor from it. The
    // cursor's init merely NAMES `xs`, so it allocates nothing and records no slot
    // — `xs`'s own untainted slot is still the release's one route, and the window
    // must anchor the release where every arm reaches it.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t xs) (if t (length xs) \
           (begin (def @cur xs) (assign cur (rest cur)) (length cur))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cur = find_binding_by_name(&hir, "cur", &arena, &symbols).expect("the cursor `cur`");
    assert!(
        arena.get(cur).is_mutated,
        "precondition: the cursor must be reassigned, or this pins nothing"
    );
    let xs = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the param `xs`");
    let xs_regions = info
        .binding_source_regions
        .get(&xs)
        .cloned()
        .unwrap_or_default();
    assert!(
        !xs_regions.is_empty()
            && info
                .binding_source_regions
                .get(&cur)
                .is_some_and(|rs| xs_regions.iter().all(|r| rs.contains(r))),
        "precondition: the cursor holds the param's regions, which is what made the \
         whole-holder reading refuse them; xs={xs_regions:?}"
    );
    for r in &xs_regions {
        assert!(
            info.frame_held_regions.contains(r),
            "r{} routes through `xs`'s own slot, which no `assign` repoints, so the \
             cursor's mutation must not refuse it; frame_held={:?}",
            r.0,
            info.frame_held_regions,
        );
    }
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a live-in value an arm walks with a cursor must be released where every arm \
         reaches it"
    );
}

#[test]
fn a_reassigned_allocating_binder_refuses_its_own_release() {
    // The refusal the reading keeps: here the mutated binding IS the route. `xs`'s
    // init allocated the region, so `region_to_slot` names `xs`'s own slot, and by
    // the release point that slot holds whatever the last `assign` stored.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t) (begin (def @xs (list 1 2 3)) \
           (if t (length xs) (begin (assign xs (rest xs)) (length xs)))))",
    );
    let allocs = find_calls_to_primitive(&hir, "list", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "one `list` literal; got {allocs:?}");
    let r = *info
        .alloc_region
        .get(&allocs[0])
        .expect("the list literal allocates a region");
    assert!(
        !info.frame_held_regions.contains(&r),
        "r{} is allocated by the init of the binding whose slot the release loads, \
         and that binding is reassigned — the route is poisoned; \
         frame_held={:?}",
        r.0,
        info.frame_held_regions,
    );
}

#[test]
fn a_value_allocated_in_an_arm_keeps_its_in_arm_release() {
    // The boundary the reading above preserves, and the one the premise exists
    // for: `x`'s allocation IS the arm, so its slot was never stored on the path
    // that skips the arm and a release at the merge would free whatever it finds.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t) (if t (let [x (list 1 2 3)] (length x)) 0))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !release_clears_the_arms(&hir, &arena, &symbols, &info, "x", &[then_id, else_id]),
        "a value allocated inside an arm must keep its release there"
    );
}

#[test]
fn a_value_born_in_an_arms_loop_keeps_its_release_inside_the_loop() {
    // The boundary the reading above preserves. `s` is allocated in the loop BODY,
    // so its release runs per iteration and its `decref_point` is a strict
    // descendant of the `While` — where one anchored release would cover N
    // allocations. Distinct from the pin above, whose region the loop only reads.
    let (hir, _arena, _symbols, info) = analyze_with_class(
        "(fn (t) (if t 0 \
           (begin (def @i 0) \
             (while (%lt i 3) (let [s (list 1 2)] (length s)) (assign i (%add i 1))) 0)))",
    );
    let loop_id = find_first(&hir, |h| {
        matches!(&h.kind, HirKind::While { .. } | HirKind::Loop { .. })
    })
    .expect("an iterative scope");
    let order = compute_order(&hir);
    let low = compute_subtree_low(&hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = low.get(&loop_id).copied().unwrap_or(0);
    let alloc = find_first(&hir, |h| matches!(&h.kind, HirKind::Call { .. }))
        .and_then(|_| {
            info.alloc_region
                .iter()
                .find(|(id, _)| {
                    let o = ord(**id);
                    o >= lo && o < ord(loop_id)
                })
                .map(|(_, &r)| r)
        })
        .expect("the loop body allocates a region");
    let dord = ord(info
        .region_data
        .get(&alloc)
        .expect("the loop-body region has RegionData")
        .decref_point);
    assert!(
        dord >= lo && dord < ord(loop_id),
        "a value born in the loop body must keep its release strictly inside the \
         loop; r{} released at order {dord}, loop body is [{lo}, {})",
        alloc.0,
        ord(loop_id),
    );
}

#[test]
fn the_match_arm_that_uses_the_value_takes_no_head_compensation() {
    // The over-free counterfactual for the arm route: the arm holding the
    // `decref_point` already releases `v` at its own last use. A head release there
    // would precede that use and free the value under it.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "v", arms[0]),
        "the arm that uses the local must not also take a head release"
    );
}

/// Does `node` carry a per-arm (`tail`) release for a region the binding named
/// `name` may point into?
fn arm_decrefs(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    node: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_arm_decrefs
        .get(&node)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// The HirIds of every `Return` node inside `[lo, hi]` of the post-order index.
fn returns_within(hir: &Hir, order: &HashMap<HirId, u32>, lo: u32, hi: u32) -> Vec<HirId> {
    find_all(hir, |h| matches!(&h.kind, HirKind::Return { .. }))
        .into_iter()
        .filter(|id| {
            let o = order.get(id).copied().unwrap_or(0);
            o >= lo && o <= hi
        })
        .collect()
}

#[test]
fn the_walk_base_case_releases_at_its_return() {
    // `(if (= i 0) xs (go (- i 1) (%pair xs 1)))` — both arms use `xs`, so the
    // recursive arm's later use takes the `decref_point` and the base case is left
    // with a return mint and no release. The release belongs at the base case's
    // `Return`, where the mint has already raised the count.
    //
    // The recursive arm STORES `xs` into a fresh cons, so escape marks it beyond the
    // return facet and the branch-arm window refuses it outright — which is what
    // leaves this route the one that discharges the base case.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(letrec [go (fn (i xs) (if (%eq i 0) xs (go (%sub i 1) (%pair xs 1))))] \
           (go 1 (list 1 2)))",
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    let order = compute_order(&hir);
    let lo = compute_subtree_low(&hir, &order)
        .get(&then_id)
        .copied()
        .expect("the then arm has a subtree interval");
    let hi = order
        .get(&then_id)
        .copied()
        .expect("the then arm is ordered");
    let rets = returns_within(&hir, &order, lo, hi);
    assert!(
        !rets.is_empty(),
        "the base-case arm must contain a Return node"
    );
    assert!(
        rets.iter()
            .any(|&n| arm_decrefs(&hir, &arena, &symbols, &info, "xs", n)),
        "the base case must release the arg at its Return, after the mint"
    );
}

// ── The env cell's compensating release ───────────────────────────────
//
// docs/impl/region/mechanism.md § "A compensating release of an env cell names the
// box, not the holder's slot". An env cell's box is minted once per activation and
// released through `LoadCaptureRaw` + `DecrefCellRegion`, which names the box
// rather than the holder's slot. Every refusal that would decline a per-arm release
// of it — a reassigned holder, a capturer's alias, the return frontier, the
// same-node retain — is a claim about the VALUE that holder names, so both routes
// carry the box: `head` where the arm names the binding nowhere, `tail` after that
// arm's last use where it reads it.

/// The env-cell region of the binding named `name` — the `cell_release_regions`
/// member among its source regions.
fn env_cell_region(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
) -> Region {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {name}"));
    info.binding_source_regions
        .get(&b)
        .into_iter()
        .flatten()
        .copied()
        .find(|r| info.cell_release_regions.contains(r))
        .unwrap_or_else(|| {
            panic!(
                "binding `{name}` must hold an env-cell region; source={:?} cell_release={:?}",
                info.binding_source_regions.get(&b),
                info.cell_release_regions,
            )
        })
}

#[test]
fn a_falling_through_arm_compensates_the_env_cell_its_sibling_relocated() {
    // `(if t (g) 0)` — `g` captures the in-lambda mutable `c`, so the box is an env
    // cell whose one `DecrefCellRegion` sits in the THEN arm (the frame-exit
    // relocation moves it ahead of that arm's `TailCall`). The ELSE arm reaches the
    // merge and names `c` nowhere, so it is a dead sibling arm and owes the head
    // release; without it the box strands once per call.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t (g) 0)))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the falling-through arm must release the env cell r{}; branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn a_reassigned_holder_does_not_withdraw_its_env_cell_compensation() {
    // The mutated face. A reassigned holder poisons a release routed through its
    // SLOT, and this release names the box no `assign` repoints — so the same head
    // release is owed. Pins that the refusal is read per region, not per holder.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (n t) (def @c n) (let [g (fn () (assign c (%add c 1)) c)] (if t (g) 0)))",
    );
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "a reassigned holder's env cell r{} must still take the head release; \
         branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn an_env_cell_takes_the_tail_route_on_the_arm_that_reads_it() {
    // `(if t c (g))` — the capture-use of `c` resolves through `g`'s last use, so
    // the box's `decref_point` follows the CALL and lands in the else arm. The then
    // arm READS `c`, making it a used sibling arm: the head route fires before the
    // arm's body and would free the box under that read. The `tail` route releases
    // after the read instead, and needs no same-node retain — the box's holders are
    // the frame's env slot plus one counted `closure ⊇ cell` edge per capturer, and
    // no use of the binding yields the box.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t c (g))))");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_arm_decrefs
            .values()
            .any(|rs| rs.contains(&cell)),
        "the arm that reads the cell's binding must release the box r{} after that \
         read; branch_arm_decrefs={:?}",
        cell.0,
        info.branch_arm_decrefs,
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !info
            .branch_compensation
            .get(&then_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the reading arm must take no head release of r{} — that frees the box \
         under its own read",
        cell.0,
    );
}

#[test]
fn an_unfunded_used_sibling_arm_takes_no_tail_route() {
    // The counterfactual that keeps the retain requirement on every other region.
    // Both arms name `xs` and neither node retains it — no store, no `-mut`
    // container, no return mint — so the arm that loses the `decref_point` max keeps
    // the conservative baseline. Only a cell release is admitted without one.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (let [n (if t (length xs) (length xs))] (%add n 1)))");
    let b = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the param `xs`");
    let regions: Vec<Region> = info
        .binding_source_regions
        .get(&b)
        .cloned()
        .unwrap_or_default();
    assert!(!regions.is_empty(), "`xs` must hold a region to judge");
    assert!(
        info.branch_arm_decrefs
            .values()
            .all(|rs| regions.iter().all(|r| !rs.contains(r))),
        "an unfunded used sibling arm must take no `tail` release; \
         regions={regions:?} branch_arm_decrefs={:?}",
        info.branch_arm_decrefs,
    );
}

#[test]
fn the_arm_that_returns_the_value_takes_no_compensation() {
    // The over-free counterfactual: the arm that DOES hand the value to the
    // caller must be left alone — its `decref_point` release, paired with the
    // mint, is the whole hand-over. A compensating release there would take the
    // caller's reference.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the returning arm must not also compensate — that frees the caller's value"
    );
}
