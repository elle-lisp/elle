// audited: 2026-10-28
//! Pins for the region walk's inline of a callee: the call names its result, the callee keeps its naming, a self call is not inlined.
//!
//! docs/impl/region/mechanism.md

use super::*;
use crate::value::SymbolId;

mod naming;

// ── A call's result is named by the call's own region ─────────────────
//
// The walk inlines a resolvable lambda callee's body so the intrinsics inside it
// record their edges at this call site. The regions that walk yields are the
// CALLEE's — minted against the callee's own nodes, remapped per activation — so
// they must not become the caller's naming for the result. The end-to-end pin is
// tests/impl/region-inline-result-naming.lisp.

/// The HirId of the first `Call` whose callee names `callee`, looking through the
/// `DerefCell` wrapper functionalization puts around a `needs_capture` read.
fn call_to(hir: &Hir, callee: &str, arena: &BindingArena) -> Option<HirId> {
    fn names(func: &Hir, callee: &str, arena: &BindingArena) -> bool {
        match &func.kind {
            HirKind::Var(b) => arena.get(*b).name == SymbolId::of(callee),
            HirKind::DerefCell { cell } => names(cell, callee, arena),
            _ => false,
        }
    }
    fn walk(hir: &Hir, callee: &str, arena: &BindingArena, found: &mut Option<HirId>) {
        if found.is_none() {
            if let HirKind::Call { func, .. } = &hir.kind {
                if names(func, callee, arena) {
                    *found = Some(hir.id);
                }
            }
        }
        hir.for_each_child(|c| walk(c, callee, arena, found));
    }
    let mut found = None;
    walk(hir, callee, arena, &mut found);
    found
}

/// How many bindings are recorded as holders of `r` — the count
/// `region::infer::compensate`'s single-holder value route reads.
fn holder_count(info: &RegionInfo, r: Region) -> usize {
    info.binding_source_regions
        .values()
        .filter(|rs| rs.contains(&r))
        .count()
}

#[test]
fn an_inlined_call_result_is_named_by_the_call_node() {
    // `mk` binds a lambda this unit can see, so the walk inlines its body. The
    // binding that holds the result must still name the CALL's own region — the
    // caller-side marker the value-resolved release resolves at runtime — and not
    // the region `mk`'s body minted for `(list 1 2)`.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(let [mk (fn () (list 1 2))] (let [v (mk)] (length v)))");
    let call = call_to(&hir, "mk", &arena).expect("a call to mk");
    let call_r = info
        .alloc_region
        .get(&call)
        .copied()
        .expect("the call node has its own region");
    let v = find_binding_by_name(&hir, "v", &arena).expect("a binding named v");
    assert_eq!(
        info.binding_source_regions.get(&v),
        Some(&vec![call_r]),
        "the result binding must name the call's own region, not the callee's"
    );
}

/// The binding named `name`, and the regions it holds.
fn held_by(hir: &Hir, name: &str, arena: &BindingArena, info: &RegionInfo) -> Vec<Region> {
    let b = find_binding_by_name(hir, name, arena).expect("the binding");
    info.binding_source_regions
        .get(&b)
        .cloned()
        .expect("the binding is a holder")
}

/// Two arms, each binding the result of its own call to one callee. Without the
/// rule both bindings name the callee's `(list 1 2)` region, and the second arm
/// is structurally later.
const TWO_ARMS: &str = "(let [mk (fn () (list 1 2))]
                          (if (cond_var)
                            (let [a (mk)] (length a))
                            (let [b (mk)] (length b))))";

#[test]
fn a_call_result_is_released_in_the_arm_that_calls() {
    // The callee's naming carries the region's one release to the later arm's
    // use, a path exclusive with the first arm's call. The first arm then
    // releases nothing.
    let (hir, arena, _symbols, info) = analyze_with_class(TWO_ARMS);
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let (lo, hi) = (order.low(&then_id).unwrap_or(0), ord(then_id));
    for r in held_by(&hir, "a", &arena, &info) {
        let d = ord(info
            .region_data
            .get(&r)
            .expect("a's region has a decref point")
            .decref_point);
        assert!(
            lo <= d && d <= hi,
            "the first arm's call result must be released in that arm, not in its sibling"
        );
    }
}

#[test]
fn an_inlined_callee_keeps_its_rest_params_collected_region() {
    // A collector puts ONE binding in both `params` and `rest_param`, so the
    // inline's save/restore visits it twice. The trap: the second visit's
    // snapshot is taken after the first visit has already overwritten the
    // entry, so restoring the snapshots in order ends on the overwritten value
    // and the rest param names nothing at all.
    //
    // The counter-factual a plain "does it still have a region" assertion would
    // miss is that nothing downstream complains: the collected keyword struct
    // still allocates, the callee still runs, and only `arena/region-count`
    // reads the difference — one region and one object per call, for every
    // `&named`/`&keys`/`&` callee this unit can resolve
    // (tests/impl/region-inline-rest-param-leak.lisp).
    let (hir, arena, _symbols, info) = analyze_with_class("(let [k (fn (&named a) 42)] (k))");
    let p = find_binding_by_name(&hir, "__named_param", &arena).expect("the named param");
    let regions = info
        .binding_source_regions
        .get(&p)
        .expect("the named param is a holder");
    assert!(
        !regions.is_empty(),
        "the collected keyword struct's region must survive the callee's inline"
    );
    assert!(
        regions
            .iter()
            .any(|r| info.region_data.contains_key(r) && info.call_result_regions.contains(r)),
        "the collected struct's region must be an owned placeholder with a release point"
    );
}

#[test]
fn an_inlined_result_region_keeps_one_holder() {
    // The second consequence of adopting the callee's naming: the region gains a
    // holder binding in the sibling arm, and a two-holder region is refused the
    // single-slot value route `region::infer::compensate` needs to place a per-arm
    // release — so the arm the misplacement stranded cannot be compensated either.
    let (hir, arena, _symbols, info) = analyze_with_class(TWO_ARMS);
    for name in ["a", "b"] {
        for r in held_by(&hir, name, &arena, &info) {
            assert_eq!(
                holder_count(&info, r),
                1,
                "a call result must be held by the binding that names that call alone"
            );
        }
    }
}

// ── A self call is not inlined ─────────────────────────────────────────
//
// A call to the lambda whose body the walk is inside starts a new activation,
// and the re-walk would name that activation's allocations by this one's
// regions (docs/impl/region/mechanism.md).

#[test]
fn a_self_call_passing_a_nested_pair_records_no_cycle_between_the_pairs() {
    // The outer pair holds the inner one, so one edge runs from the inner
    // pair's region to the outer's. Inlining the self call re-walks `go`'s body
    // and stores the argument, the outer pair, into the inner pair's site: an
    // edge the other way, and a cycle between two regions of ONE activation.
    // The co-owned group cut then frees both pairs before the call that
    // receives the outer one.
    let (hir, info) = analyze_full(
        "(defn go [lst acc]
           (if (%pair? lst)
             (go (%rest lst) (%pair (%first lst) (%pair 0 acc)))
             acc))
         (go (list 1 2 3) ())",
    );
    let mut pairs = Vec::new();
    find_all_pairs_helper(&hir, &mut pairs);
    let [outer, inner] = pairs[..] else {
        panic!("expected the outer and the inner %pair, found {pairs:?}");
    };
    let region = |id: HirId| {
        info.alloc_region
            .get(&id)
            .copied()
            .expect("each %pair has a region")
    };
    let (outer, inner) = (region(outer), region(inner));
    let edge = |from: Region, to: Region| {
        info.cross_region_refs
            .iter()
            .any(|&(_, f, t)| f == from && t == to)
    };
    assert!(edge(inner, outer), "the outer pair holds the inner one");
    assert!(
        !edge(outer, inner),
        "the inner pair must not hold the outer one: the self call is a new activation"
    );
}
