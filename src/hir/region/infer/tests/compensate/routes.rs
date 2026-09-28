// audited: 2026-09-28
//! Pins the per-arm compensation routes: `head` on a dead sibling arm, `tail` after a
//! used arm's read, none on the arm that returns.
//!
//! docs/impl/region/compensate.md
//!
//! A returned region is the caller's to free only on the paths that hand it over.
//! On a sibling arm that never uses the value no return mint fires, the caller
//! receives nothing, and the callee still holds the only reference — so that arm
//! owes a compensating release even though escape marks the region returnable.
//! tests/impl/region-return-arm-escape-leak.lisp pins it end to end.

use super::*;

#[test]
fn the_match_arm_that_uses_the_value_takes_no_head_compensation() {
    // The over-free counter-factual for the arm route: the arm holding the
    // `decref_point` already releases `v` at its own last use. A head release there
    // would precede that use and free the value under it.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert!(
        !arm_compensates(&hir, &arena, &info, "v", arms[0]),
        "the arm that uses the local must not also take a head release"
    );
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
    let (hir, arena, _symbols, info) = analyze_with_class(
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
            .any(|&n| arm_decrefs(&hir, &arena, &info, "xs", n)),
        "the base case must release the arg at its Return, after the mint"
    );
}

#[test]
fn an_unfunded_used_sibling_arm_takes_no_tail_route() {
    // The counter-factual that keeps the retain requirement on every other region.
    // Both arms name `xs` and neither node retains it — no store, no `-mut`
    // container, no return mint — so the arm that loses the `decref_point` max keeps
    // the conservative baseline. Only a cell release is admitted without one.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (let [n (if t (length xs) (length xs))] (%add n 1)))");
    let b = find_binding_by_name(&hir, "xs", &arena).expect("the param `xs`");
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
    // The over-free counter-factual: the arm that DOES hand the value to the
    // caller must be left alone — its `decref_point` release, paired with the
    // mint, is the whole hand-over. A compensating release there would take the
    // caller's reference.
    let (hir, arena, _symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", then_id),
        "the returning arm must not also compensate — that frees the caller's value"
    );
}
