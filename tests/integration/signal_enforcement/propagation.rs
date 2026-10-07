// audited: 2026-10-06
// How a signal propagates: from a yield to its body, through calls and bindings, and past an `assign`.
//
// docs/signals/inference.md

use super::*;

// ============================================================================
// 1. DIRECT YIELD SIGNAL TESTS
// ============================================================================

#[test]
fn test_signal_direct_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn () (yield 1))", &mut symbols, &mut vm, "<test>").unwrap();

    // Creating a lambda is silent
    assert_eq!(result.hir.signal, Signal::silent());

    // But the body should be Yields
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.signal, Signal::yields());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_signal_yield_in_begin() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (yield 1) (yield 2))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(result.hir.signal, Signal::yields());
}

#[test]
fn test_signal_yield_in_if() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(if true (yield 1) 2)", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(result.hir.signal, Signal::yields());
}

// ============================================================================
// 2. CALL PROPAGATION TESTS
// ============================================================================

#[test]
fn test_signal_call_propagation() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def gen (fn () (yield 1))) (gen))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Calling a yielding function should propagate Yields signal"
    );
}

#[test]
fn test_signal_nested_propagation() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def gen (fn () (yield 1))) (def wrapper (fn () (gen))) (wrapper))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Nested call to yielding function should propagate Yields signal"
    );
}

#[test]
fn test_signal_silent_call() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def f (fn (x) (%add x 1))) (f 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::silent(),
        "Calling a silent intrinsic function propagates the silent signal"
    );
}

#[test]
fn test_signal_let_bound_lambda() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [gen (fn () (yield 1))] (gen))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Calling a let-bound yielding lambda should propagate Yields signal"
    );
}

#[test]
fn test_signal_letrec_bound_lambda() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(letrec [gen (fn () (yield 1))] (gen) 42)",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Calling a letrec-bound yielding lambda should propagate Yields signal"
    );
}

// ============================================================================
// 3. HIGHER-ORDER AND UNKNOWN-GLOBAL CALL TESTS
// ============================================================================

#[test]
fn test_signal_polymorphic_local_higher_order() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f lst)                (if (empty? lst)                    ()                    (%pair (f (first lst)) (my-map f (rest lst))))))            (def gen (fn (x) (yield x)))            (my-map gen (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // my-map is polymorphic on param 0 (with inherent error).
    // Calling with gen (which yields) resolves to yields + errors.
    assert_eq!(
        result.hir.signal,
        Signal::yields_errors(),
        "Local higher-order function with yielding arg resolves to yields+errors"
    );
}

#[test]
fn test_signal_unknown_global_call_is_unknown() {
    // `map` is a stdlib function, and this analysis loads no stdlib, so `map`
    // is an unknown global → Signal::unknown()
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(map (fn (x) (%add x 1)) (list 1 2 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "Call to unknown global is Signal::unknown() (sound)"
    );
}

#[test]
fn test_signal_unknown_global_call_with_yielding_arg_is_unknown() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "Call to unknown global is Signal::unknown() (sound)"
    );
}

// ============================================================================
// 4. ASSIGN INVALIDATION TESTS
// ============================================================================

#[test]
fn test_signal_assign_makes_the_callee_unknown() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (var f (fn () 42)) (assign f (fn () (yield 1))) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "After assign, callee signal is unknown (sound)"
    );
}

// A `var` that SHADOWS a primitive name, once reassigned, must resolve calls
// to Signal::unknown() — never the shadowed primitive's signal.
//
// The counter-factual: a callee lookup that falls back to the primitive
// signals by name would give a reassigned local named `length` the signal of
// the `length` primitive, not the sound conservative `unknown`.
#[test]
fn test_assign_invalidated_local_ignores_shadowed_global_signal() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (var length (fn () 42)) (assign length (fn () (yield 1))) (length))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "Reassigned local shadowing global `length` must be unknown, not the global's signal"
    );
}

// ============================================================================
// 5. DIRECT LAMBDA CALL TESTS
// ============================================================================

#[test]
fn test_signal_direct_lambda_call_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("((fn () (yield 1)))", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Direct call to yielding lambda should have Yields signal"
    );
}

#[test]
fn test_signal_direct_lambda_call_silent() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("((fn () 42))", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::silent(),
        "Direct call to a silent lambda should be silent"
    );
}

// ============================================================================
// 6. COMPLEX SCENARIOS
// ============================================================================

#[test]
fn test_signal_silent_then_yielding_calls_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def pure-fn (fn () 42)) (def yield-fn (fn () (yield 1))) (pure-fn) (yield-fn))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "A silent call followed by a yielding call should have Yields signal"
    );
}

#[test]
fn test_signal_closure_captures_yielding() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [gen (fn () (yield 1))] (let [wrapper (fn () (gen))] (wrapper)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Nested closure calling yielding function should have Yields signal"
    );
}
