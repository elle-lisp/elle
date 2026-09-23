// audited: 2026-09-22
// What the naming rule does with a propagating tail — the body a `let`,
// `letrec`, `loop` or `parameterize` hands its own value up from — and with the
// returning positions such tails lead out of.
//
// src/hir/anf.rs

use super::*;
use crate::hir::expr::HirKind;

// ── 8. tail call in let body keeps is_tail marker ────────────

#[test]
fn tail_call_in_let_body_keeps_is_tail_marker() {
    // `(fn () (let [a 1] (g a)))` — (g a) is the body of the let,
    // which is the tail of the lambda. mark_tail_calls runs before
    // anf_lift; the is_tail flag on (g a) must survive.
    //
    // If ANF wraps the tail Call (as `(let [t (g a)] t)`), the
    // wrapped Call still has is_tail=true even though it's no longer
    // syntactically in tail position — body_is_tail_call recognizes
    // the wrap shape as tail-equivalent.
    let (hir, arena, symbols) = analyze_anf("((fn () (let [a 1] (g a))))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert!(!g_calls.is_empty(), "expected at least one call to g");
    for call_node in &g_calls {
        if let HirKind::Call { is_tail, .. } = &call_node.kind {
            assert!(
                *is_tail,
                "Call(g) was tail before ANF; must remain tail after"
            );
        }
    }
}

// ── 8b. a propagating tail is named through ───────────────────

#[test]
fn allocation_in_a_let_body_is_named_in_a_consumer_position() {
    // `(g (let [a 1] (f a)))`. The `let` allocates nothing at its own id, so a
    // name on the `let` records no region and the call's result reaches the
    // lowerer with no release route. The name belongs on `(f a)`.
    //
    // Counter-factual: reading the position instead of the node passes as long
    // as SOMETHING named the argument — and the wrap the old rule promised
    // ("the outer consumer wraps the form itself") is exactly the wrap that
    // names nothing.
    let (hir, arena, symbols) = analyze_anf("(g (let [a 1] (f a)))");
    let f_call = only_call_to(&hir, "f", &arena, &symbols).id;
    assert!(
        named_node_ids(&hir).contains(&f_call),
        "the let body's call must carry the name, not the let"
    );
}

#[test]
fn allocation_in_a_let_body_is_named_under_a_binder() {
    // `(let [b (let [a 1] (f a))] (g b))`. `b` names the value, and names it
    // uselessly: `record_region_slot` keys `b`'s slot on what the INIT node
    // allocates, and the inner `let` allocates nothing. So the inner call is
    // named too, by the one rule, and `b`'s own slot is left to the shape it
    // does cover — an init that allocates at its own id.
    let (hir, arena, symbols) = analyze_anf("(let [b (let [a 1] (f a))] (g b))");
    let f_call = only_call_to(&hir, "f", &arena, &symbols).id;
    assert!(
        named_node_ids(&hir).contains(&f_call),
        "a binder whose init is a propagating tail must name the tail"
    );
}

#[test]
fn a_lambda_body_tail_is_not_named() {
    // The one tail the rule leaves alone. `(fn () (let [a 1] (f a)))` hands
    // `(f a)`'s value to the CALLER, which releases it through its own binding;
    // this frame owes no release, so it needs no name — and naming a tail call
    // would rebuild it as the let-bound form for nothing.
    let (hir, arena, symbols) = analyze_anf("(g (fn () (let [a 1] (f a))))");
    let f_call = only_call_to(&hir, "f", &arena, &symbols).id;
    assert!(
        !named_node_ids(&hir).contains(&f_call),
        "a lambda body's tail value leaves by the return mint, unnamed"
    );
}

// ── 8c. a returning position names what the frame must release ──

/// The single `Eval` node in the tree.
fn only_eval<'a>(hir: &'a Hir) -> &'a Hir {
    let mut found = Vec::new();
    let mut visit = |node: &'a Hir| {
        if matches!(node.kind, HirKind::Eval { .. }) {
            found.push(node);
        }
    };
    walk_pre(hir, &mut visit);
    assert_eq!(found.len(), 1, "expected exactly one eval");
    found[0]
}

#[test]
fn a_call_at_the_root_is_named() {
    // The fixture's root is `(letrec [stubs] (f 1))`, so `(f 1)` is the value
    // the unit returns. `mark_tail_calls` marks no call at the top level, so it
    // comes back as an owned result AND takes the `Return` mint.
    //
    // Counter-factual: unnamed, the call's reference has no slot to leave
    // through, and each run of the unit strands the result's region. That is
    // every `(eval '(f …))`, once per eval.
    let (hir, arena, symbols) = analyze_anf("(f 1)");
    let f_call = only_call_to(&hir, "f", &arena, &symbols);
    assert!(
        matches!(f_call.kind, HirKind::Call { is_tail: false, .. }),
        "a call at the root is never a tail call"
    );
    assert!(
        named_node_ids(&hir).contains(&f_call.id),
        "the root's call result must carry a name"
    );
}

#[test]
fn a_call_in_a_let_body_at_the_root_is_named() {
    // The root descends its propagating tails exactly as a consumer does, so
    // the name lands on the call, not on the `let` that hands its value up.
    let (hir, arena, symbols) = analyze_anf("(let [a 1] (f a))");
    let f_call = only_call_to(&hir, "f", &arena, &symbols).id;
    assert!(
        named_node_ids(&hir).contains(&f_call),
        "the root's let-body call must carry the name"
    );
}

#[test]
fn an_eval_at_a_lambda_tail_is_named() {
    // An `Eval` is never a tail call, so at a lambda tail its owned result
    // takes the `Return` mint on top of the reference the eval handed back.
    //
    // Counter-factual: the lambda-body tail was left unnamed as a whole, which
    // is right for a tail call and wrong here — `(fn () (eval x))` then strands
    // one region per call.
    let (hir, _arena, _symbols) = analyze_anf("(g (fn () (eval 1)))");
    let eval = only_eval(&hir).id;
    assert!(
        named_node_ids(&hir).contains(&eval),
        "an eval at a lambda tail must carry a name"
    );
}

#[test]
fn an_eval_in_a_let_body_at_a_lambda_tail_is_named() {
    let (hir, _arena, _symbols) = analyze_anf("(g (fn () (let [a 1] (eval a))))");
    let eval = only_eval(&hir).id;
    assert!(
        named_node_ids(&hir).contains(&eval),
        "a lambda tail descends its let body to the eval"
    );
}

#[test]
fn a_call_in_a_parameterize_body_at_a_lambda_tail_is_named() {
    // A `parameterize` body is never a tail position — its frame pops after
    // the body — so the call there is not a tail call, and its value still
    // leaves through the lambda's `Return` mint.
    let (hir, arena, symbols) = analyze_anf("(g (fn () (parameterize ((h 1)) (f 2))))");
    let f_call = only_call_to(&hir, "f", &arena, &symbols);
    assert!(
        matches!(f_call.kind, HirKind::Call { is_tail: false, .. }),
        "a parameterize body is never a tail position"
    );
    assert!(
        named_node_ids(&hir).contains(&f_call.id),
        "the parameterize body's call must carry a name"
    );
}

#[test]
fn a_fresh_allocation_at_a_lambda_tail_is_not_named() {
    // The rule names only what hands this frame an owning reference. A fresh
    // allocation's region already has its own release, so it keeps the shape
    // it had.
    let (hir, _arena, _symbols) = analyze_anf("(g (fn () (%pair 1 2)))");
    let mut named_intrinsic = false;
    for id in named_node_ids(&hir) {
        if let Some(node) = find_node(&hir, id) {
            named_intrinsic |= matches!(node.kind, HirKind::Intrinsic { .. });
        }
    }
    assert!(
        !named_intrinsic,
        "a fresh allocation at a lambda tail stays unnamed"
    );
}
