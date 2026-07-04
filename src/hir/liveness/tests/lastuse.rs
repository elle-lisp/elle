use super::*;

#[test]
fn last_use_let_single_var_in_body() {
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (let [x (string \"a\")] x))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
    let alloc = allocs[0];

    let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
    assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
    let var = var_uses[0];
    // Post-ANF the let body `x` is wrapped in a synthetic `Return(Var(x))`;
    // the alloc's last_use is keyed off that wrap (same death site as the
    // bare Var, one level up). Accept the use leaf or its ANF wrap.
    let wrap = find_parent(&hir, var);

    let got = info.last_use.get(&alloc).copied();
    assert!(
        got == Some(var) || got == wrap,
        "alloc @{} should die at Var(x) @{} (or its ANF wrap @{:?}), got {:?}",
        alloc.0,
        var.0,
        wrap.map(|w| w.0),
        got
    );
}

#[test]
fn last_use_let_multiple_uses_in_body() {
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (begin x x)))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1);
    let alloc = allocs[0];

    let mut var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
    assert_eq!(var_uses.len(), 2, "expected two Var(x) uses");
    // The last use is the one whose live_out has no further reference to x.
    // Source order: first Var(x) earlier, second Var(x) later. The last
    // syntactic Var(x) is the second.
    var_uses.sort_by_key(|id| id.0);
    let last = *var_uses.last().unwrap();
    // Post-ANF the tail `x` (the second use) is wrapped in `Return(Var(x))`;
    // last_use is keyed off that wrap. Accept the last use leaf or its wrap.
    let wrap = find_parent(&hir, last);

    let got = info.last_use.get(&alloc).copied();
    assert!(
        got == Some(last) || got == wrap,
        "alloc @{} should die at the second Var(x) @{} (or its ANF wrap @{:?}), got {:?}",
        alloc.0,
        last.0,
        wrap.map(|w| w.0),
        got
    );
}

#[test]
fn last_use_inline_call_arg_no_binding() {
    // `(string (string "a"))` — the inner string allocation has no
    // binding; its value flows directly into the outer string call.
    // Last use is at the outer Call. `string` is a real primitive
    // so the analyzer does not inline these calls (unlike the
    // letrec-bound `g` in the test wrapper).
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (string (string \"a\")))");

    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2, "expected two (string ...) calls");
    // The inner call has the lower HirId; the outer call wraps it.
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let inner = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&inner).copied();
    assert_eq!(
        got,
        Some(outer),
        "inline alloc @{} should have last_use at the consuming Call @{}, got {:?}",
        inner.0,
        outer.0,
        got
    );
}

#[test]
fn last_use_emit_yield() {
    // The yielded value's last use is the Emit node — the runtime
    // incref at handle_emit keeps the region alive past
    // the matching DecrefRegion.
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (emit :yield (string \"a\")))");

    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
    let alloc = allocs[0];

    let emit = find_first_emit(&hir).expect("expected an Emit node");

    let got = info.last_use.get(&alloc).copied();
    assert_eq!(
        got,
        Some(emit),
        "emit-yielded alloc @{} should have last_use at Emit @{}, got {:?}",
        alloc.0,
        emit.0,
        got
    );
}

#[test]
fn last_use_across_nested_let() {
    // `(let [x (string "a")] (let [y 1] x))` — Var(x) lives inside the
    // inner let. last_use of the alloc must be that Var(x), not the
    // outer let's exit.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (let [x (string \"a\")] (let [y 1] x)))");

    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1);
    let alloc = allocs[0];

    let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
    assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
    let var = var_uses[0];
    // Post-ANF the inner-let body `x` is wrapped in `Return(Var(x))`,
    // still inside the inner let — so last_use keys off that wrap, NOT
    // the outer let's exit. Accept the use leaf or its ANF wrap.
    let wrap = find_parent(&hir, var);

    let got = info.last_use.get(&alloc).copied();
    assert!(
        got == Some(var) || got == wrap,
        "nested-let alloc @{} should die at inner Var(x) @{} (or its ANF wrap @{:?}), got {:?}",
        alloc.0,
        var.0,
        wrap.map(|w| w.0),
        got
    );
}

// ── propagation through Or/And/If/Let body/Begin tail ───────────
//
// Invariant: when a propagating form (Or/And, If/Cond/Match branches,
// Let/Letrec/Loop body, Begin tail, Block tail, Parameterize body)
// is consumed by an outer call, the propagating form's tail children
// must see THAT outer call as their last-use, not the propagating
// form itself. A call-result region whose `decref_point` is set too early
// releases its slot before the outer consumer reads it — the slot's
// memory then gets reused for the next allocation and the stale
// Value's tag bits no longer match the heap object's discriminant.
// (Surfaced at tests/elle/telemetry.lisp:135 via
// `@{:attrs (or attrs {})}` — see tests/elle/bug-propagate-free-at.lisp.)

#[test]
fn last_use_or_propagates_to_outer_consumer() {
    // (string (or true (string "x")))
    // The inner (string "x") flows up through `or` to the outer
    // (string ...) Call; its last_use must be the outer Call.
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (string (or true (string \"x\"))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2);
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let inner = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&inner).copied();
    assert_eq!(
        got,
        Some(outer),
        "(or _ (string ...)) inner alloc @{} should free at outer Call @{}, got {:?}",
        inner.0,
        outer.0,
        got
    );
}

#[test]
fn last_use_and_propagates_to_outer_consumer() {
    // (string (and true (string "x")))
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (string (and true (string \"x\"))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2);
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let inner = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&inner).copied();
    assert_eq!(
        got,
        Some(outer),
        "(and _ (string ...)) inner alloc @{} should free at outer Call @{}, got {:?}",
        inner.0,
        outer.0,
        got
    );
}

#[test]
fn last_use_if_branch_propagates_to_outer_consumer() {
    // (string (if true (string "a") (string "b")))
    // Both branches' allocs flow to the outer (string ...) Call.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (string (if true (string \"a\") (string \"b\"))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 3, "outer + then + else");
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let then_alloc = sorted[0];
    let else_alloc = sorted[1];
    let outer = sorted[2];

    for (label, branch) in [("then", then_alloc), ("else", else_alloc)] {
        let got = info.last_use.get(&branch).copied();
        assert_eq!(
            got,
            Some(outer),
            "{} branch alloc @{} should free at outer Call @{}, got {:?}",
            label,
            branch.0,
            outer.0,
            got
        );
    }
}

#[test]
fn last_use_let_body_propagates_to_outer_consumer() {
    // (string (let [y 1] (string "x")))
    // The inner (string "x") is the let body's tail; it flows up
    // to the outer (string ...) — its last_use must be the outer Call.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (string (let [y 1] (string \"x\"))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2);
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let inner = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&inner).copied();
    assert_eq!(
        got,
        Some(outer),
        "let-body alloc @{} should free at outer Call @{}, got {:?}",
        inner.0,
        outer.0,
        got
    );
}

#[test]
fn last_use_begin_tail_propagates_to_outer_consumer() {
    // (string (begin 1 (string "x")))
    // Begin's last expr is the tail; flows up to the outer Call.
    let (hir, arena, symbols, info) = analyze_with_hir("(fn () (string (begin 1 (string \"x\"))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2);
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let inner = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&inner).copied();
    assert_eq!(
        got,
        Some(outer),
        "begin-tail alloc @{} should free at outer Call @{}, got {:?}",
        inner.0,
        outer.0,
        got
    );
}

#[test]
fn last_use_begin_non_tail_dies_at_statement_boundary() {
    // (string (begin (string "discarded") "ret"))
    //
    // The first (string "discarded") is a statement; its value is
    // discarded. Its region must die at the statement boundary —
    // NOT propagate up to the outer Call.
    //
    // Post-ANF, `Hir::for_each_child` shows the discarded Call
    // wrapped in a synthetic `Let([t = Call], Var(t))`. The Call's
    // last_use is the Let's id (the wrap); the Let's id is
    // inside the Begin (well below the outer Call). The region
    // release fires at the Let's id via `region_to_slot`, keyed off
    // the wrap binding's slot — statement-boundary semantics.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (string (begin (string \"discarded\") \"ret\")))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 2);
    let mut sorted = allocs.clone();
    sorted.sort_by_key(|id| id.0);
    let discarded = sorted[0];
    let outer = sorted[1];

    let got = info.last_use.get(&discarded).copied();
    assert!(
        got.is_some_and(|id| id != outer),
        "begin-statement alloc @{} must NOT die at the outer Call @{} \
             (the statement value flows to a wrap binding, not to the \
              outer consumer). got={:?}",
        discarded.0,
        outer.0,
        got
    );
}

// ── propagation through iterative scopes (While / Loop) ─────────
//
// A binding bound OUTSIDE a `while` body but referenced INSIDE the
// body must outlive the entire while — not die at the immediate
// consumer inside the body. Otherwise the per-iteration decref of
// the binding's region triggers UAF on iteration 2 (the canonical
// symptom that surfaces as the phantom-region panic on
// tests/elle/jit-lbox-param-repro.lisp).
//
// Counterfactual: with the current `walk` for While (`walk(body,
// false, hir.id)`), uses inside the body have last_use set to the
// immediate consumer (e.g., the Call's HirId), which is strictly
// less than the While's HirId. The binding-chain extension then
// sets `last_use[init_id] = call.id`, leaking the bug into
// regions analysis (`r.decref_point = call.id`, inside the while body).
//
// Fix: when walking a use inside a While/Loop body, the effective
// last_use for binding-extension purposes must be at LEAST the
// While/Loop's HirId (or anything that survives a single iteration).
