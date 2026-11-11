// audited: 2026-11-10
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
    // `(if (%eq i 0) xs (begin (git xs) (go (%sub i 1) (%pair xs 1))))` — both
    // arms use `xs`, so the recursive arm's later use takes the `decref_point` and
    // the base case is left with a return mint and no release. The release belongs
    // at the base case's `Return`, where the mint has already raised the count.
    //
    // The recursive arm also hands `xs` to `git`, a `Mixed` native that stores it
    // uncounted, so escape marks it contained and the branch-arm window refuses it
    // outright. That refusal leaves this route the one that discharges the base
    // case. The fresh cons alone would not: it counts its hold on `xs`, so the
    // window would take the release to the merge instead.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(letrec [go (fn (i xs) (if (%eq i 0) xs \
                   (begin (git xs) (go (%sub i 1) (%pair xs 1)))))] \
           (go 1 (list 1 2)))",
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    let order = compute_order(&hir);
    let lo = order
        .low(&then_id)
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

#[test]
fn a_dead_arm_compensates_a_self_recursive_def() {
    // `dl` refers to itself through a `Recursive` self-edge, whose env slot holds
    // nil, so no environment holds the closure. The else arm tail-calls it, which
    // puts the closure's one release past that `TailCall`, and the then arm never
    // names it. The counter-factual: the region capture graph counted the
    // self-edge as a capture, which tainted the region out of compensation, and
    // the then arm leaked the closure once per call (docs/impl/selfrec.md).
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
        arm_compensates(&hir, &arena, &info, "dl", then_id),
        "the arm that never names `dl` must release its closure at its head"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "dl", else_id),
        "the arm that tail-calls `dl` keeps the deferred release and takes no head release"
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
