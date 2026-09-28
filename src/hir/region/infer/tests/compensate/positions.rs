// audited: 2026-09-28
//! Pins the arms of `cond`, `and` and `or`: a clause test and a short-circuit tail are
//! conditional positions like any arm body.
//!
//! docs/impl/region/window.md

use super::*;

// ── An arm is a conditional position, not a syntactic arm body ───────
//
// A `cond`'s clause TESTS are conditional positions exactly as its bodies are,
// and an `and`/`or` tail is one with no body at all. Reading those forms
// syntactically leaves a release whose last use is a later test outside every
// arm — which is where the polymorphic entry point puts it (see
// docs/impl/region/window.md and the end-to-end rows in
// tests/impl/region-branch-arm-window.lisp).

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
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (cond (%eq t 0) 1 (%lt 0 (length xs)) 2 true 0))");
    let parts = first_cond_parts(&hir).expect("a Cond node");
    assert_eq!(parts.len(), 6, "three clauses, each a test and a body");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &parts),
        "a live-in param whose last use is a later clause's TEST must be released \
         where every clause reaches it"
    );
}

#[test]
fn a_cond_body_is_an_arm_like_any_other() {
    // The body half of the same decomposition: `xs`'s last use is the LAST clause's
    // body, so every earlier clause strands it. `Cond` is a branch, so its bodies
    // are arms exactly as an `If`'s and a `Match`'s are.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (cond (%eq t 0) 1 (%eq t 1) (length xs) true 0))");
    let parts = first_cond_parts(&hir).expect("a Cond node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &parts),
        "a live-in param used by one clause body must be released where every \
         clause reaches it"
    );
}

#[test]
fn a_short_circuit_tail_is_an_arm() {
    // `(or a b)` evaluates `b` only where `a` is falsy, so `b` is a conditional
    // position with no sibling body — a one-armed branch. `xs`'s last use sits
    // there, and the path that short-circuits must still release it.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (if (or t (%lt 0 (length xs))) 1 2))");
    let parts = first_short_circuit_parts(&hir).expect("an Or node");
    assert_eq!(parts.len(), 2, "the `or` has two elements");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &parts[1..]),
        "a live-in param whose last use is a short-circuited tail must be released \
         where both paths reach it"
    );
}

#[test]
fn an_and_tail_is_an_arm_too() {
    // The `and` face of the same rule: the tail runs only where the head is truthy.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (if (and t (%lt 0 (length xs))) 1 2))");
    let parts = first_short_circuit_parts(&hir).expect("an And node");
    assert_eq!(parts.len(), 2, "the `and` has two elements");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &parts[1..]),
        "a live-in param whose last use is a short-circuited `and` tail must be \
         released where both paths reach it"
    );
}

#[test]
fn an_arm_whose_loop_reads_a_live_in_param_anchors_at_the_branch() {
    // The window's iterative boundary is the loop's BODY, not the loop's own node.
    // A read of a loop-external binding is anchored at the loop NODE
    // (docs/impl/region/anchors.md), and the
    // lowerer emits a node's releases after it, so that release already runs once
    // per execution of the loop — the same count with which the merge is reached.
    // Reading the boundary as the closed subtree interval would leave the branch's
    // only release under the looping arm, stranding `xs` on every other arm.
    let (hir, arena, _symbols, info) = analyze_with_class(
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
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
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
    // (docs/impl/region/window.md). `w` here is such a
    // binding, so `xs` is still live-in and its one release moves to the merge.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (t xs) (if t (length xs) (let [w xs] (length w))))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "an alias the arm introduces must not read as a birth in the arm"
    );
    assert!(
        !arm_compensates(&hir, &arena, &info, "xs", then_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}
