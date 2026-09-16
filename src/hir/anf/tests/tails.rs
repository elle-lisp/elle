// audited: 2026-09-16
// What the naming rule does with a propagating tail — the body a `let`,
// `letrec`, `loop` or `parameterize` hands its own value up from.
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
