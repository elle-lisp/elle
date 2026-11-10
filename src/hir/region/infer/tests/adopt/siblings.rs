// audited: 2026-11-09
//! Sibling closures over one captured value: rooted at the outer closure, and refused when a sibling outlives it.
//!
//! docs/impl/region/adopt.md

use super::*;

/// `f` and `g` both capture `b`, and `f` captures `g`. Nothing outlives `(f)`, so the
/// subtree `{f, g, b}` is rooted at `f`, and `f` adopts both captures. The adopt
/// edges must be emittable as well as admitted.
///
/// Counter-factual: with a binder's init last used at the binder node, `b` and `g`
/// were held one step past `f`'s drop, and the obligation refused the subtree to
/// Shared.
#[test]
fn sibling_capture_adopted_by_the_outer_closure() {
    let (hir, info, edges) = adopt_edges(
        "(begin (let [b (%pair 1 2)] \
                  (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] (f))) \
                nil)",
    );
    let b = sole_pair_region(&hir, &info);
    let adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    let owner = adopts
        .iter()
        .find(|&&(m, _)| m == b)
        .map(|&(_, o)| o)
        .unwrap_or_else(|| panic!("the captured pair r{} is adopted; got {adopts:?}", b.0));
    assert_eq!(
        adopts.len(),
        2,
        "the outer closure r{} adopts the pair and its sibling; got {adopts:?}",
        owner.0,
    );
    for (member, closure) in adopts {
        assert_eq!(
            closure, owner,
            "one root adopts every member; got r{}",
            closure.0
        );
        assert!(
            closure_captures_region(&hir, &info, member, closure),
            "the adopt of r{} by r{} has no capture to reload it from",
            member.0,
            closure.0,
        );
    }
}

/// The same siblings, with `g` called again after `f` returns. `g` and `b` then
/// outlive the root's drop, so the obligation refuses the subtree and every member
/// keeps its own release.
///
/// Counter-factual: a cut that claimed sibling captures by shape alone would free
/// `g` and `b` at `f`'s drop, under the second call of `g`.
#[test]
fn sibling_capture_called_after_the_outer_closure_is_refused() {
    let (_, _, edges) = adopt_edges(
        "(begin (let [b (%pair 1 2)] \
                  (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] \
                    (begin (f) (g)))) \
                nil)",
    );
    assert!(
        edges.capture.is_empty() && edges.store.is_empty(),
        "a sibling called after the root dies refuses the subtree; got capture={:?} \
         store={:?}",
        edges.capture,
        edges.store,
    );
}
