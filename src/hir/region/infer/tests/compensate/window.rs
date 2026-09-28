// audited: 2026-09-28
//! Pins the branch-arm release window: where one release every arm reaches replaces the
//! in-arm release, and where it declines.
//!
//! docs/impl/region/window.md
//! docs/impl/region/relocate.md
//! docs/impl/region/replicate.md

use super::*;

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
    // other handed nothing over (docs/impl/region/window.md). So the one release
    // moves to the branch and neither compensation route fires.
    let (hir, arena, _symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a returned param's release must sit where both arms reach it"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_returned_param_anchors_whichever_arm_carries_it_out() {
    // The mirror: `xs` leaves through the ELSE arm. Pins that the admission reads
    // arm structure and not arm position.
    let (hir, arena, _symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) 7 xs))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a returned param's release must sit where both arms reach it"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", then_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_frame_exit_the_callee_cannot_reach_anchors_a_returned_param() {
    // The ELSE arm leaves through a callee that neither names `xs` nor captures it.
    // A callee reaches a value this frame owns as an operand or through its captured
    // environment and by no other route, so this one cannot name `xs` at all — its
    // `Return` mints nothing against that region and the replica ahead of the
    // `TailCall` is the region's last release (docs/impl/region/relocate.md). So the
    // branch anchors the return facet here exactly as it does where the callee
    // captures.
    let (hir, arena, _symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs (g 7)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a frame exit the callee cannot reach must not decline the returned param"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", else_id)
            && !arm_compensates(&hir, &arena, &info, "xs", then_id),
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
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(begin (def step (fn (f n i acc) \
           (if (%lt i n) (step f n (%add i 1) (f acc i)) acc))) (step g 2 0 nil))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "acc", &[then_id, else_id]),
        "the fold driver's accumulator must be released where both arms reach it"
    );
}

#[test]
fn read_only_arm_release_clears_the_arms() {
    // Control: when no arm carries `xs` across the return frontier the same
    // anchoring applies. Guards against a change that treats the returned shape
    // specially by dropping the baseline.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) 7))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a merely-read param's release must sit where both arms reach it"
    );
}

#[test]
fn match_arms_are_treated_like_if_arms() {
    // Every premise here is stated over ONE ARM and its siblings — never over the
    // branch's arity or kind. `v` is allocated before the dispatch, so it is
    // live-in on every arm and no arm may hold its only release.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert_eq!(arms.len(), 3, "the dispatch has three arms");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "v", &arms),
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
    // (docs/impl/region/window.md). Only a VALUE route is replicable, and a call
    // result is value-routed unconditionally — which is what the first assertion
    // states about this shape and the second relies on.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (g 7)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let b = find_binding_by_name(&hir, "xs", &arena).expect("the param `xs`");
    assert!(
        info.binding_source_regions.get(&b).is_some_and(
            |rs| !rs.is_empty() && rs.iter().all(|r| info.call_result_regions.contains(r))
        ),
        "the narrowing admits only value-routed regions, so this pin needs `xs` \
         to be one — a region released by id keeps the whole-branch decline"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a frame-replacing sibling arm must not decline a value-routed release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_frame_replacing_arm_anchors_a_binder_routed_release() {
    // The same branch shape, over a region the lowerer releases by ID unless some
    // point admits it: `xs` is a `%pair` allocation the `let` binder owns, so it
    // is no call result. `record_region_slot` still keyed a slot on that
    // allocation, so the relocation can take the value route and replicate the
    // release into the frame-replacing arm (docs/impl/region/replicate.md). The
    // window asks that question rather than reading the region's class, so this
    // branch narrows to `xs` instead of declining it.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (i) (let [xs (%pair 1 nil)] (if (%eq i 0) (length xs) (g 7))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let b = find_binding_by_name(&hir, "xs", &arena).expect("the binding `xs`");
    assert!(
        info.binding_source_regions.get(&b).is_some_and(
            |rs| !rs.is_empty() && rs.iter().all(|r| !info.call_result_regions.contains(r))
        ),
        "the discriminator: this pin is about a region OUTSIDE `call_result_regions`, \
         which is what the class reading declined"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a frame-replacing sibling arm must not decline a binder-routed release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", then_id)
            && !arm_compensates(&hir, &arena, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_binders_allocation_is_value_routed() {
    // The positive half of the mirror, stated where the window reads it: an
    // ordinary `let` binder's slot holds its init's value from the binder to the
    // release, so the region that init allocated can be released by value and
    // replicated into an arm.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (i) (let [xs (%pair 1 nil)] (if (%eq i 0) (length xs) (g 7))))");
    let routes = binder_routes(&hir, &arena, &info, "xs");
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
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (i) (let [@xs (%pair 1 nil) f (fn () (length xs))] \
           (if (%eq i 0) (%add (length xs) (f)) (g 7))))",
    );
    let routes = binder_routes(&hir, &arena, &info, "xs");
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
    // (docs/impl/region/relocate.md). Anchoring it at the merge would take it out
    // of that channel's reach and leave the arm with no release at all, so the
    // branch declines it.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (xs) (letrec [go (fn (a b) a)] (if (%eq xs 0) 0 (go xs 1))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let routes = binder_routes(&hir, &arena, &info, "go");
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
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (@i n) (let [@xs (%pair 1 nil)] \
           (begin (while (%lt i n) (begin (assign xs (%pair i xs)) (assign i (%add i 1)))) \
             (if (%eq i 0) (length xs) (g 7)))))",
    );
    let routes = binder_routes(&hir, &arena, &info, "xs");
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
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (dst n) (if (%eq n 0) dst \
           (letrec [go (fn (i) (if (%lt i n) (go (%add i 1)) dst))] (go 0))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "dst", &[then_id, else_id]),
        "a capture-funded frame exit must not decline the returned param"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "dst", then_id)
            && !arm_compensates(&hir, &arena, &info, "dst", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_native_tail_arm_does_not_decline_the_window() {
    // A tail call to a NATIVE pushes no frame and falls through to the merge, so
    // it is not a frame exit at all and the narrowing above never applies — the
    // distinction is the callee kind, not `is_tail`.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (length xs)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a native tail call in an arm must not decline the window"
    );
}
