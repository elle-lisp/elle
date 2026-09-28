// audited: 2026-09-28
//! Pins the mutated refusal: only a reassignment of the binding that owns the release
//! route refuses the window.
//!
//! docs/impl/region/window.md

use super::*;

// ── The mutated refusal is about the route, and one binding owns it ──
//
// `region_to_slot` is keyed on a region's allocation site, so the slot a
// value-routed release loads belongs to the binding whose init allocated the
// region — or, where nothing in this body allocates it, to the parameter the
// lambda prologue recorded. Every other holder names the same value through a slot
// no release reads, so the mutated question is asked of the route's binding alone
// (docs/impl/region/window.md). The pins below are the admissions and the refusals
// they keep.

#[test]
fn a_reassigned_destructured_name_refuses_nothing() {
    // The binder forms that record a route are `Define`, `Let`/`Letrec` and the
    // lambda prologue, and no others. A DESTRUCTURING name is none of them: the
    // pattern extracts it from the scrutinee, and the lowerer records no
    // `region_to_slot` entry for it, so no value-routed release can load the slot
    // its `assign` repoints. The destructured list lives in the ANF temp that
    // produced it, whose own slot is bound once and never repointed — reassigning
    // one of the names the pattern introduced must not refuse it.
    let (hir, arena, _symbols, info) =
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
            .map(|&b| arena.get(b).name)
            .collect::<Vec<_>>()
    );
    let a = mutated[0];
    assert_eq!(
        arena.get(a).name,
        SymbolId::of("a"),
        "precondition: the reassigned binding is the destructured `a`"
    );
    let allocs = find_calls_to_primitive(&hir, "list", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t @p) (begin (assign p (rest p)) (if t (length p) 0)))");
    let p = find_binding_by_name(&hir, "p", &arena).expect("the param `p`");
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
    let calls = find_calls_to_primitive(&hir, "rest", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (t xs) (if t (length xs) \
           (begin (def @cur xs) (assign cur (rest cur)) (length cur))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cur = find_binding_by_name(&hir, "cur", &arena).expect("the cursor `cur`");
    assert!(
        arena.get(cur).is_mutated,
        "precondition: the cursor must be reassigned, or this pins nothing"
    );
    let xs = find_binding_by_name(&hir, "xs", &arena).expect("the param `xs`");
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
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a live-in value an arm walks with a cursor must be released where every arm \
         reaches it"
    );
}

#[test]
fn a_reassigned_allocating_binder_refuses_its_own_release() {
    // The refusal the reading keeps: here the mutated binding IS the route. `xs`'s
    // init allocated the region, so `region_to_slot` names `xs`'s own slot, and by
    // the release point that slot holds whatever the last `assign` stored.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (t) (begin (def @xs (list 1 2 3)) \
           (if t (length xs) (begin (assign xs (rest xs)) (length xs)))))",
    );
    let allocs = find_calls_to_primitive(&hir, "list", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t) (if t (let [x (list 1 2 3)] (length x)) 0))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !release_clears_the_arms(&hir, &arena, &info, "x", &[then_id, else_id]),
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
