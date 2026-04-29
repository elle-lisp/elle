// Integration tests for interprocedural signal tracking and enforcement
//
// These tests verify that signals propagate correctly across function boundaries:
// - Direct yield has Yields signal
// - Calling a yielding function propagates Yields signal
// - Polymorphic signals (like map) resolve based on argument signals
// - Silent functions remain silent
// - assign invalidates signal tracking
// - Unknown callees use Signal::unknown() (sound conservative)
// - Parameter calls use Signal::yields_errors() (may yield + inherent error)

use elle::hir::HirKind;
use elle::pipeline::analyze;
use elle::primitives::register_primitives;
use elle::signals::{Signal, SIG_ERROR};
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// ============================================================================
// 1. DIRECT YIELD EFFECT TESTS
// ============================================================================

#[test]
fn test_signal_direct_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn () (yield 1))", &mut symbols, &mut vm, "<test>").unwrap();

    // Lambda creation is pure
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
fn test_signal_pure_call() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def f (fn (x) (+ x 1))) (f 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Calling an error-capable function propagates the error signal"
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
// 3. POLYMORPHIC EFFECT RESOLUTION TESTS
// ============================================================================

#[test]
fn test_signal_polymorphic_local_higher_order() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f lst)                (if (empty? lst)                    ()                    (pair (f (first lst)) (my-map f (rest lst))))))            (def gen (fn (x) (yield x)))            (my-map gen (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
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
fn test_signal_polymorphic_direct_call() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-fn (fn (f x) (f x)))            (apply-fn (fn (x) (yield x)) 42))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // apply-fn is polymorphic(0) with inherent error.
    // Called with a yielding lambda → yields + errors.
    assert_eq!(
        result.hir.signal,
        Signal::yields_errors(),
        "Higher-order function with yielding arg resolves to yields+errors"
    );
}

#[test]
fn test_signal_polymorphic_with_pure_arg() {
    // map isn't in primitive_signals (defined in stdlib, not a primitive).
    // Unknown global → Signal::unknown()
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(map (fn (x) (+ x 1)) (list 1 2 3))",
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
fn test_signal_polymorphic_with_yielding_arg_unknown_global() {
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
fn test_signal_set_invalidation() {
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
fn test_signal_direct_lambda_call_pure() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("((fn () 42))", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::silent(),
        "Direct call to pure lambda should be Pure"
    );
}

// ============================================================================
// 6. COMPLEX SCENARIOS
// ============================================================================

#[test]
fn test_signal_multiple_calls_mixed() {
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
        "After assign, callee is unknown"
    );
}

#[test]
fn test_signal_conditional_yield() {
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
        "Call to function with conditional yield should have Yields signal"
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

// ============================================================================
// 7. PRIMITIVE EFFECT TESTS
// ============================================================================

#[test]
fn test_signal_pure_primitives() {
    let (mut symbols, mut vm) = setup();

    let errors_calls = [
        "(+ 1 2)", "(- 5 3)", "(* 2 3)", "(/ 10 2)",
        "(< 1 2)", "(> 2 1)", "(= 1 1)",
        "(pair 1 2)", "(first (list 1 2))", "(rest (list 1 2))",
        "(length (list 1 2 3))", "(not true)",
        "(number? 42)", "(string? \"hello\")",
    ];

    for call in errors_calls {
        let result = analyze(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::errors(),
            "Primitive call '{}' should have errors signal (type/arity checks)",
            call
        );
    }

    let inert_calls = ["(list 1 2 3)"];
    for call in inert_calls {
        let result = analyze(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::silent(),
            "Primitive call '{}' should be silent",
            call
        );
    }
}

// ============================================================================
// 8. LAMBDA BODY EFFECT TRACKING
// ============================================================================

#[test]
fn test_lambda_body_signal_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (yield x))", &mut symbols, &mut vm, "<test>").unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.signal, Signal::yields());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_signal_nested_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (begin (+ x 1) (yield x) (+ x 2)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.signal, Signal::yields_errors());
    } else {
        panic!("Expected Lambda");
    }
}

// ============================================================================
// 9. UNKNOWN GLOBAL SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_signal_unknown_global_is_yields() {
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
        "Unknown global should be Signal::unknown() for soundness"
    );
}

// ============================================================================
// 10. UNKNOWN CALLEE SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_signal_parameter_call_is_yields() {
    // Calling a function parameter: yields_errors() (may yield + inherent error)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (f) (f 42))", &mut symbols, &mut vm, "<test>").unwrap();
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(
            body.signal,
            Signal::yields_errors(),
            "Calling a function parameter has yields_errors() signal"
        );
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_signal_let_bound_non_lambda_call_is_yields() {
    // Calling a let-bound non-lambda: Signal::unknown() (opaque binding)
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [f (first fns)] (f 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "Calling a let-bound non-lambda should be Signal::unknown() (opaque)"
    );
}

// ============================================================================
// 11. AUTOMATIC POLYMORPHIC EFFECT INFERENCE TESTS
// ============================================================================

#[test]
fn test_polymorphic_inference_single_param() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def apply-fn (fn (f x) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_signals, ..
        } = &value.kind
        {
            // Polymorphic on param 0 with inherent SIG_ERROR (calling unknown can error)
            assert_eq!(
                *inferred_signals,
                Signal::polymorphic_errors(0),
                "apply-fn should have Polymorphic(0) + errors signal"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_polymorphic_inference_resolves_pure() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def apply-fn (fn (f x) (f x))) (apply-fn + 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Calling polymorphic function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_resolves_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def apply-fn (fn (f x) (f x))) (apply-fn (fn (x) (yield x)) 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields_errors(),
        "Calling polymorphic function with yielding arg should be Yields+Errors"
    );
}

#[test]
fn test_polymorphic_inference_my_map() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f xs)              (if (empty? xs) (list)                  (pair (f (first xs)) (my-map f (rest xs))))))           (my-map + (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Recursive higher-order function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_non_recursive_map() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-to-list (fn (f xs)              (if (empty? xs) (list)                  (pair (f (first xs)) (list)))))           (apply-to-list + (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Non-recursive higher-order function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_direct_yield_prevents() {
    // A function that both calls a parameter AND yields directly is Yields+Errors
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def bad (fn (f x) (begin (yield 99) (f x))))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_signals, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_signals,
                Signal::yields_errors(),
                "Function with direct yield + param call should be Yields+Errors"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_polymorphic_inference_two_params() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def apply-both (fn (f g x) (begin (f x) (g x))))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_signals, ..
        } = &value.kind
        {
            // Polymorphic on params 0 and 1, with inherent SIG_ERROR
            assert_eq!(
                *inferred_signals,
                Signal {
                    bits: SIG_ERROR,
                    propagates: 0b11,
                },
                "Function calling two params should propagate params 0 and 1 + error"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_polymorphic_inference_two_params_resolves_pure() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def apply-both (fn (f g x) (begin (f x) (g x)))) (apply-both + * 5))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Calling polymorphic function with errors args should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_two_params_resolves_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def gen (fn () (yield 1)))           (def apply-both (fn (f g x) (begin (f x) (g x))))            (apply-both gen * 5))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::yields_errors(),
        "Calling Polymorphic({{0,1}}) with one yielding arg should be Yields+Errors"
    );
}

#[test]
fn test_polymorphic_inference_second_param() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def apply-second (fn (x f) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_signals, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_signals,
                Signal::polymorphic_errors(1),
                "apply-second should have Polymorphic(1) + errors signal"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_polymorphic_inference_with_known_yielding_call() {
    // Function that calls a parameter AND a known yielding function.
    // The known yielding call makes it non-polymorphic (has_non_param_yield = true)
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def gen (fn () (yield 1))) (def bad (fn (f x) (begin (gen) (f x)))))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    // Result is a Begin; bad is the last Define
    if let HirKind::Begin(exprs) = &result.hir.kind {
        let bad_def = exprs.last().unwrap();
        if let HirKind::Define { value, .. } = &bad_def.kind {
            if let HirKind::Lambda {
                inferred_signals, ..
            } = &value.kind
            {
                assert_eq!(
                    *inferred_signals,
                    Signal::yields_errors(),
                    "Function with known yielding call + param call should be Yields+Errors"
                );
            } else {
                panic!("Expected Lambda in Define");
            }
        } else {
            panic!("Expected Define as last expr in Begin");
        }
    } else {
        panic!("Expected Begin, got {:?}", result.hir.kind);
    }
}
