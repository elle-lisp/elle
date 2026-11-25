// audited: 2026-11-24
//! Pins the branch-arm window's callee: a region an exiting arm tail-calls is anchored, and each call
//! that names it records the release.
//!
//! docs/impl/region/window.md
//! docs/impl/region/relocate.md

use super::*;

/// Every tail call whose callee is a read of a binding named `name`.
fn tail_calls_to(hir: &Hir, arena: &BindingArena, name: &str) -> Vec<HirId> {
    fn walk(h: &Hir, arena: &BindingArena, name: &str, out: &mut Vec<HirId>) {
        if let HirKind::Call {
            func,
            is_tail: true,
            ..
        } = &h.kind
        {
            if let HirKind::Var(b) = &func.kind {
                if arena.get(*b).name == SymbolId::of(name) {
                    out.push(h.id);
                }
            }
        }
        h.for_each_child(|c| walk(c, arena, name, out));
    }
    let mut out = Vec::new();
    walk(hir, arena, name, &mut out);
    out
}

/// Does the window's record at `call` name every region in `regions`?
fn recorded_at(info: &RegionInfo, call: HirId, regions: &[Region]) -> bool {
    info.anchored_callee_calls
        .get(&call)
        .is_some_and(|rs| regions.iter().all(|r| rs.contains(r)))
}

#[test]
fn a_callee_the_arm_tail_calls_anchors_and_is_recorded() {
    // `go`'s own closure region is what the exiting arm's call names as its
    // CALLEE. The window anchors it like any other region, and the call records
    // it, so the deferred callee channel still runs the release on the closure
    // path (docs/impl/region/window.md). The counter-factual keeps the callee's
    // in-arm release, which an arm that calls it elsewhere never reaches.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (xs) (letrec [go (fn (a b) a)] (if (%eq xs 0) 0 (go xs 1))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let routes = binder_routes(&hir, &arena, &info, "go");
    assert!(
        routes
            .iter()
            .all(|(_, r)| info.value_routed_regions.contains(r)),
        "the discriminator: {routes:?} is binder-routed, so only the callee \
         exemption could decline it"
    );
    assert!(
        routes.iter().all(|(_, r)| region_release_clears_the_arms(
            &hir,
            &info,
            *r,
            &[then_id, else_id]
        )),
        "a tail callee's own closure region must take the anchored release; got \
         {routes:?}"
    );
    let calls = tail_calls_to(&hir, &arena, "go");
    let regions: Vec<Region> = routes.iter().map(|&(_, r)| r).collect();
    assert_eq!(calls.len(), 1, "the else arm tail-calls `go` once");
    assert!(
        recorded_at(&info, calls[0], &regions),
        "the call that leaves through `go` must record {regions:?}; got {:?}",
        info.anchored_callee_calls
    );
}

#[test]
fn every_arm_that_tail_calls_the_callee_is_recorded() {
    // Both arms tail-call the parameter `b`, so its one in-arm release reached
    // one arm and the other stranded `b`. Anchored, the release runs at the merge
    // on the native path, and each call records it for the closure path.
    let (hir, arena, _symbols, info) = analyze_with_class("(fn (b t) (if (%eq t 0) (b 1) (b 2)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "b", &[then_id, else_id]),
        "the callee both arms tail-call must take the anchored release"
    );
    let b = find_binding_by_name(&hir, "b", &arena).expect("the parameter `b`");
    let regions = info.binding_source_regions[&b].clone();
    let calls = tail_calls_to(&hir, &arena, "b");
    assert_eq!(calls.len(), 2, "each arm tail-calls `b`");
    for call in calls {
        assert!(
            recorded_at(&info, call, &regions),
            "the call @{} must record {regions:?}; got {:?}",
            call.0,
            info.anchored_callee_calls
        );
    }
}

#[test]
fn a_dead_arm_reaches_the_anchored_release_of_a_self_recursive_def() {
    // `dl` refers to itself through a `Recursive` self-edge, whose env slot holds
    // nil, so no environment holds the closure. The else arm tail-calls it, and
    // the then arm never names it. The window anchors the closure's release at
    // the merge, which the then arm reaches, so neither arm takes a head release
    // (docs/impl/selfrec.md). The counter-factual keeps the release past the else
    // arm's `TailCall`, where the then arm needs a head release of its own.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (c xs) (begin (def dl (fn (l) (if (f l) l (dl (g l))))) (if c 1 (dl xs))))",
    );
    let c = find_binding_by_name(&hir, "c", &arena).expect("the param `c`");
    let outer = find_all(
        &hir,
        |h| matches!(&h.kind, HirKind::If { cond, .. } if matches!(cond.kind, HirKind::Var(b) if b == c)),
    );
    let [outer] = outer.as_slice() else {
        panic!("expected one If on `c`, found {outer:?}");
    };
    let (then_id, else_id) = if_arms_of(&hir, *outer).expect("the outer If's arms");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "dl", &[then_id, else_id]),
        "the closure an arm tail-calls must take the anchored release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "dl", then_id)
            && !arm_compensates(&hir, &arena, &info, "dl", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
    let b = find_binding_by_name(&hir, "dl", &arena).expect("the binding `dl`");
    let regions = info.binding_source_regions[&b].clone();
    assert!(
        tail_calls_to(&hir, &arena, "dl")
            .into_iter()
            .any(|call| recorded_at(&info, call, &regions)),
        "the else arm's call must record {regions:?}; got {:?}",
        info.anchored_callee_calls
    );
}

/// The `(then_id, else_id)` of the `If` node `id`, searched from `hir`.
fn if_arms_of(hir: &Hir, id: HirId) -> Option<(HirId, HirId)> {
    if hir.id == id {
        if let HirKind::If {
            then_branch,
            else_branch,
            ..
        } = &hir.kind
        {
            return Some((then_branch.id, else_branch.id));
        }
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = if_arms_of(c, id);
        }
    });
    found
}
