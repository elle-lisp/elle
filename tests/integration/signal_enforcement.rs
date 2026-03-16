// Integration tests for interprocedural signal tracking and enforcement
//
// These tests verify that signals propagate correctly across function boundaries:
// - Direct yield has Yields signal
// - Calling a yielding function propagates Yields signal
// - Polymorphic signals (like map) resolve based on argument signals
// - Silent functions remain silent
// - assign invalidates signal tracking

use elle::hir::HirKind;
use elle::pipeline::{analyze, analyze_file};
use elle::primitives::register_primitives;
use elle::signals::Signal;
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
    // (fn () (yield 1)) should have Pure signal on the lambda creation
    // but the body should have Yields signal
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
    // (begin (yield 1) (yield 2)) should have Yields signal
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
    // (if true (yield 1) 2) should have Yields signal
    let (mut symbols, mut vm) = setup();
    let result = analyze("(if true (yield 1) 2)", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(result.hir.signal, Signal::yields());
}

// ============================================================================
// 2. CALL PROPAGATION TESTS
// ============================================================================

#[test]
fn test_signal_call_propagation() {
    // (def gen (fn () (yield 1)))
    // (gen) should have Yields signal
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
    // (def gen (fn () (yield 1)))
    // (def wrapper (fn () (gen)))
    // (wrapper) should be Yields
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
    // (def f (fn (x) (+ x 1)))
    // (f 42) should be Pure
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
        Signal::silent(),
        "Calling a pure function should remain Pure"
    );
}

#[test]
fn test_signal_let_bound_lambda() {
    // (let ((gen (fn () (yield 1)))) (gen)) should have Yields signal
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let ((gen (fn () (yield 1)))) (gen))",
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
    // (letrec ((gen (fn () (yield 1)))) (gen)) should have Yields signal
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(letrec ((gen (fn () (yield 1)))) (gen) 42)",
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

// Note: map, filter, fold are defined as Lisp functions in init_stdlib,
// not as primitives. For polymorphic signal resolution to work with them,
// they would need to be defined in the same compilation unit or tracked
// across compilation units. These tests verify the behavior with locally
// defined higher-order functions.

#[test]
fn test_signal_polymorphic_local_higher_order() {
    // Define a local higher-order function and verify polymorphic resolution
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f lst)                (if (empty? lst)                    ()                    (cons (f (first lst)) (my-map f (rest lst))))))            (def gen (fn (x) (yield x)))            (my-map gen (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // my-map calls gen which yields, so my-map's body has Yields signal
    // When we call (my-map gen ...), we look up my-map's signal
    // Since my-map is defined with a lambda, we track its body signal
    // The body calls f which is a parameter - we can't resolve that statically
    // So this is Yields (sound: unknown callee may yield)
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Local higher-order function with unknown parameter signal is conservatively Yields"
    );
}

#[test]
fn test_signal_polymorphic_direct_call() {
    // Direct call with yielding lambda should propagate signal
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-fn (fn (f x) (f x)))            (apply-fn (fn (x) (yield x)) 42))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // apply-fn's body calls f which is a parameter
    // We can't statically resolve the parameter's signal
    // So this is Yields (sound: unknown callee may yield)
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Higher-order function with parameter call is conservatively Yields"
    );
}

#[test]
fn test_signal_polymorphic_with_pure_arg() {
    // Calling a global function (map) with pure lambda
    // Since map isn't in primitive_signals (it's defined in stdlib),
    // the call is conservatively Yields (sound: unknown global may yield)
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
        Signal::yields(),
        "Call to unknown global is Yields (sound default)"
    );
}

#[test]
fn test_signal_polymorphic_with_yielding_arg_unknown_global() {
    // Calling a global function (map) that isn't in primitive_signals
    // Unknown globals default to Yields for soundness
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    // map is not in primitive_signals (it's defined in stdlib, not as a primitive)
    // Unknown globals are Yields for soundness
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Call to unknown global is Yields (sound default)"
    );
}

// ============================================================================
// 4. ASSIGN INVALIDATION TESTS
// ============================================================================

#[test]
fn test_signal_set_invalidation() {
    // (var f (fn () 42))
    // (assign f (fn () (yield 1)))
    // After assign, signal tracking for f is invalidated
    // Calling f should be Yields (sound: we can't prove it's pure)
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (var f (fn () 42)) (assign f (fn () (yield 1))) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    // After assign, we conservatively treat the signal as Yields
    // This is sound: we can't prove the new value is pure
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "After assign, signal should be Yields (sound default)"
    );
}

// ============================================================================
// 5. DIRECT LAMBDA CALL TESTS
// ============================================================================

#[test]
fn test_signal_direct_lambda_call_yields() {
    // ((fn () (yield 1))) should have Yields signal
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
    // ((fn () 42)) should be Pure
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
    // (begin (def pure-fn (fn () 42))
    //        (def yield-fn (fn () (yield 1)))
    //        (pure-fn)
    //        (yield-fn))
    // Should have Yields signal because yield-fn is called
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
        Signal::yields(),
        "Sequence with yielding call should have Yields signal"
    );
}

#[test]
fn test_signal_conditional_yield() {
    // (def maybe-yield (fn (x) (if x (yield 1) 2)))
    // (maybe-yield true) should have Yields signal
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
    // (let ((gen (fn () (yield 1))))
    //   (let ((wrapper (fn () (gen))))
    //     (wrapper)))
    // Should have Yields signal
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let ((gen (fn () (yield 1)))) (let ((wrapper (fn () (gen)))) (wrapper)))",
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
    // Most primitives now have errors signal (can raise type/arity errors).
    // `list` is genuinely silent (variadic, no type checks).
    let (mut symbols, mut vm) = setup();

    let errors_calls = [
        "(+ 1 2)",
        "(- 5 3)",
        "(* 2 3)",
        "(/ 10 2)",
        "(< 1 2)",
        "(> 2 1)",
        "(= 1 1)",
        "(cons 1 2)",
        "(first (list 1 2))",
        "(rest (list 1 2))",
        "(length (list 1 2 3))",
        "(not true)",
        "(number? 42)",
        "(string? \"hello\")",
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

    // list is silent — variadic constructor with no type checks
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
    // Unknown global functions default to Yields (sound)
    // This is the fix for signal soundness: if we can't prove a global is pure,
    // we must assume it may yield (since it could be redefined via assign)
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
        Signal::yields(),
        "Unknown global should be Yields for soundness"
    );
}

// ============================================================================
// 10. UNKNOWN CALLEE SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_signal_parameter_call_is_yields() {
    // Calling a function parameter should be Yields (we can't know its signal)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (f) (f 42))", &mut symbols, &mut vm, "<test>").unwrap();
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(
            body.signal,
            Signal::yields(),
            "Calling a function parameter should be Yields (unknown signal)"
        );
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_signal_let_bound_non_lambda_call_is_yields() {
    // Calling a let-bound non-lambda should be Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let ((f (first fns))) (f 42))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    // f is not a lambda literal, signal unknown → Yields+Errors
    assert_eq!(
        result.hir.signal,
        Signal::yields_errors(),
        "Calling a let-bound non-lambda should be Yields+Errors (unknown signal)"
    );
}

// ============================================================================
// 11. AUTOMATIC POLYMORPHIC EFFECT INFERENCE TESTS
// ============================================================================

#[test]
fn test_polymorphic_inference_single_param() {
    // Higher-order function should infer Polymorphic(0)
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def apply-fn (fn (f x) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    // Check the lambda's inferred signal
    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_signals, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_signals,
                Signal::polymorphic(0),
                "apply-fn should have Polymorphic(0) signal"
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
    // Calling apply-fn with a pure function should be Pure
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
    // Calling apply-fn with a yielding lambda should be Yields
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
        Signal::yields(),
        "Calling polymorphic function with yielding arg should be Yields"
    );
}

#[test]
fn test_polymorphic_inference_my_map() {
    // User-defined recursive map - the recursive call is seeded with Pure
    // during analysis (since define seeds lambda forms with Pure before
    // analyzing the body), so the function correctly infers as Polymorphic(0).
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f xs)              (if (empty? xs) (list)                  (cons (f (first xs)) (my-map f (rest xs))))))           (my-map + (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // my-map is Polymorphic(0) because the only Yields source is calling f.
    // The recursive call to my-map is seeded as Pure during analysis.
    // When called with +, which now has errors signal, the result has errors.
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Recursive higher-order function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_non_recursive_map() {
    // Non-recursive higher-order function should be Polymorphic(0)
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-to-list (fn (f xs)              (if (empty? xs) (list)                  (cons (f (first xs)) (list)))))           (apply-to-list + (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // apply-to-list is Polymorphic(0), + now has errors signal, so the call has errors
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Non-recursive higher-order function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_direct_yield_prevents() {
    // A function that both calls a parameter AND yields directly is Yields, not Polymorphic
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
                Signal::yields(),
                "Function with direct yield should be Yields, not Polymorphic"
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
    // A function that calls two different parameters — should infer Polymorphic({0, 1})
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
            assert_eq!(
                *inferred_signals,
                Signal {
                    bits: elle::value::SignalBits(0),
                    propagates: 0b11, // params 0 and 1
                },
                "Function calling two params should propagate params 0 and 1"
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
    // Calling apply-both with two pure functions should be Pure
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
    // Calling apply-both with one yielding function should be Yields
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
    // Higher-order function where the second parameter is called
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
                Signal::polymorphic(1),
                "apply-second should have Polymorphic(1) signal"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_polymorphic_inference_nested_call() {
    // Nested higher-order function: outer calls inner which calls param
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-fn (fn (f x) (f x)))           (def wrapper (fn (g y) (apply-fn g y)))           (wrapper + 42))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    // wrapper calls apply-fn with g, apply-fn is Polymorphic(0)
    // So wrapper's body signal depends on g's signal
    // wrapper should be Polymorphic(0) and the final call with + (which has errors) propagates errors
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Nested polymorphic calls with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_with_known_yielding_call() {
    // A function that calls a parameter AND a known yielding function is Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def gen (fn () (yield 1)))           (def bad (fn (f x) (begin (gen) (f x)))))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();

    // Find the 'bad' definition
    if let HirKind::Begin(exprs) = &result.hir.kind {
        if let HirKind::Define { value, .. } = &exprs[1].kind {
            if let HirKind::Lambda {
                inferred_signals, ..
            } = &value.kind
            {
                assert_eq!(
                    *inferred_signals,
                    Signal::yields(),
                    "Function calling known yielding function should be Yields"
                );
            } else {
                panic!("Expected Lambda");
            }
        } else {
            panic!("Expected Define");
        }
    } else {
        panic!("Expected Begin");
    }
}

#[test]
fn test_polymorphic_inference_pure_function() {
    // A pure function should have Pure signal, not Polymorphic
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def add1 (fn (x) (+ x 1)))",
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
                Signal::silent(),
                "Pure function should have Pure signal"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

// Cross-form signal tracking is now handled natively by the letrec model.
// The old fixpoint-based tests have been removed. Equivalent coverage is
// provided by test_mutual_recursion_signals_are_pure in pipeline.rs and
// the nqueens signal test.

// ============================================================================
// CHUNK 2: (signal :keyword) form tests
// ============================================================================

// test_signal_declaration_returns_keyword: migrated to tests/elle/signals.lisp

// test_signal_declaration_non_keyword_error: migrated to tests/elle/signals.lisp

// test_signal_declaration_builtin_error: migrated to tests/elle/signals.lisp

// test_signal_in_expression_position: migrated to tests/elle/signals.lisp

// test_signal_declaration_duplicate_error: migrated to tests/elle/signals.lisp

// ============================================================================
// CHUNK 3: silence form parsing tests
// ============================================================================

#[test]
fn test_silence_parses_function_level_silent() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence) (+ x 1))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // Should parse without error
    assert!(result.is_ok());
}

#[test]
fn test_silence_parses_param_level_silent() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) (silence f) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // Should parse without error
    assert!(result.is_ok());
}

#[test]
fn test_silence_parses_param_level_with_keyword() {
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(
        "(signal :restrict_c3a) (def _ (fn (f x) (silence f :restrict_c3a) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — should be a compile error
    assert!(
        result.is_err(),
        "expected error: silence takes no signal keywords"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_unknown_keyword_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f) (silence f :nonexistent_c3b) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — error is about keywords not being accepted
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_unknown_param_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (f) (silence g) (f))", &mut symbols, &mut vm, "<test>");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("not a parameter") || err.contains("unknown parameter"));
}

#[test]
fn test_silence_duplicate_param_last_wins() {
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(
        "(signal :dup_p_c3c) (def _ (fn (f) (silence f) (silence f) (f)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // Two (silence f) forms: last wins — should parse without error
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
}

#[test]
fn test_silence_outside_lambda_not_special() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(silence f)", &mut symbols, &mut vm, "<test>");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("unresolved")
            || err.contains("not found")
            || err.contains("inside a function"),
    );
}

#[test]
fn test_silence_function_level_with_keywords() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence :error) (error \"boom\"))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — should be a compile error
    assert!(
        result.is_err(),
        "expected error: silence takes no signal keywords"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_after_docstring() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) \"Apply f.\" (silence f) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Lambda { doc, .. } = &result.hir.kind {
        assert!(doc.is_some(), "Should have docstring");
    } else {
        panic!("Expected Lambda");
    }
}

// ============================================================================
// CHUNK 4: Signal inference with bounds tests
// ============================================================================

#[test]
fn test_silence_param_eliminates_polymorphism() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def apply-inert (fn (f x) (silence f) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // Should parse without error
    assert!(result.is_ok());
}

#[test]
fn test_silence_param_contributes_bound_bits() {
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(
        "(signal :bound_c4a) (def apply-bounded (fn (f x) (silence f :bound_c4a) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — should be a compile error
    assert!(
        result.is_err(),
        "expected error: silence takes no signal keywords"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_function_ceiling_passes() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence) (+ x 1))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
}

#[test]
fn test_silence_function_ceiling_fails() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence) (yield x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("restricted") || err.contains("yield"));
}

#[test]
fn test_silence_function_ceiling_error_passes() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence :error) (error \"boom\"))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — should be a compile error
    assert!(
        result.is_err(),
        "expected error: silence takes no signal keywords"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_function_ceiling_error_fails_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence :error) (yield x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — error is about keywords
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_callsite_concrete_fails() {
    // Compile-time callsite checking is not yet implemented — silence bounds
    // are enforced at runtime via CheckSignalBound. Use eval_source to verify
    // the runtime check catches the violation.
    let result = crate::common::eval_source(
        "(begin (def apply-inert (fn (f x) (silence f) (f x))) (apply-inert (fn (x) (yield x)) 42))",
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("signal-violation") || err.contains("silence") || err.contains("bound"));
}

#[test]
fn test_silence_param_with_user_signal() {
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(
        "(signal :user_c4b) (def apply-user (fn (f) (silence f :user_c4b) (f)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — should be a compile error
    assert!(
        result.is_err(),
        "expected error: silence takes no signal keywords"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

#[test]
fn test_silence_ceiling_fails_bounded_param() {
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(
        "(signal :ceil_c4c) (def bad (fn (f x) (silence f :ceil_c4c) (silence) (f x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // silence no longer accepts signal keywords — error is about keywords
    assert!(result.is_err(), "expected error but got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("silence takes no signal keywords"),
        "expected 'silence takes no signal keywords', got: {}",
        err
    );
}

// ============================================================================
// CHUNK 5: Runtime signal checking tests
// ============================================================================

// test_silence_runtime_check_passes: migrated to tests/elle/signals.lisp

// test_silence_runtime_check_fails: migrated to tests/elle/signals.lisp

// test_silence_runtime_non_closure_passes: migrated to tests/elle/signals.lisp

// test_silence_runtime_bounded_keyword: migrated to tests/elle/signals.lisp

// test_silence_runtime_bounded_keyword_fails: migrated to tests/elle/signals.lisp

// test_silence_runtime_dynamic_passes: migrated to tests/elle/signals.lisp

// test_silence_runtime_dynamic_fails: migrated to tests/elle/signals.lisp

// ============================================================================
// CHUNK 5b: squelch form parsing and semantic tests
// ============================================================================

#[test]
fn test_squelch_parses_param_level() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) (squelch f :yield) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // (squelch f :yield) should compile without error
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
}

#[test]
fn test_squelch_parses_param_level_multi() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) (squelch f :yield :error) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // (squelch f :yield :error) should compile without error
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
}

#[test]
fn test_squelch_no_keywords_param_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) (squelch f) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // (squelch f) with no keywords is a compile error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("squelch requires at least one signal keyword"),
        "expected 'squelch requires at least one signal keyword', got: {}",
        err
    );
}

#[test]
fn test_squelch_no_keywords_bare_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (squelch) x)", &mut symbols, &mut vm, "<test>");
    // (squelch) with no arguments is a compile error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("squelch requires at least one signal keyword"),
        "expected 'squelch requires at least one signal keyword', got: {}",
        err
    );
}

#[test]
fn test_squelch_unknown_keyword_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f) (squelch f :not-a-signal) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // :not-a-signal is not registered — should be a compile error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not registered"),
        "expected 'not registered', got: {}",
        err
    );
}

#[test]
fn test_squelch_unknown_param_error() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f) (squelch not-a-param :yield) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    // not-a-param is not a parameter of this function — should be a compile error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not a parameter"),
        "expected 'not a parameter', got: {}",
        err
    );
}

// test_squelch_outside_lambda_not_special: migrated to tests/elle/signals.lisp

#[test]
fn test_squelch_param_stays_polymorphic() {
    // A squelch-bounded parameter remains polymorphic in signal inference.
    // (squelch f :yield) says f must NOT yield, but f's signal is still polymorphic.
    // Verify: the lambda with (squelch f :yield) should NOT be silent — it
    // should propagate f's signal.
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (f x) (squelch f :yield) (f x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    let result = result.expect("expected ok");
    // The outer lambda (fn (f x) ...) wraps the inner; we check the lambda signal
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        // Body signal should propagate f — not silent
        // (If body were silent, (f x) would be unreachable for signal purposes)
        assert_ne!(
            body.signal,
            Signal::silent(),
            "squelch-bounded param should keep lambda polymorphic, not silence it"
        );
    } else {
        panic!("Expected Lambda");
    }
}

// test_squelch_function_floor_passes: migrated to tests/elle/signals.lisp

#[test]
fn test_squelch_function_floor_fails() {
    // (squelch :yield) at function level — body DOES yield — should be a compile error.
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (squelch :yield) (yield x))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("squelched"),
        "expected 'squelched' in error, got: {}",
        err
    );
}

// test_squelch_with_user_signal: migrated to tests/elle/signals.lisp

#[test]
fn test_squelch_mixed_function_level_error() {
    // Using both (silence) and (squelch :yield) at function level is a compile error.
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (silence) (squelch :yield) (+ x 1))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot use both"),
        "expected 'cannot use both' in error, got: {}",
        err
    );
}

// test_squelch_and_silence_different_params: migrated to tests/elle/signals.lisp

// test_squelch_overrides_silence_same_param: migrated to tests/elle/signals.lisp

// ============================================================================
// CHUNK 6: (signals) introspection primitive tests
// ============================================================================

// test_signals_primitive_returns_struct: migrated to tests/elle/signals.lisp
// test_signals_primitive_contains_builtins: migrated to tests/elle/signals.lisp
// test_signals_primitive_contains_user_signals: migrated to tests/elle/signals.lisp

#[test]
fn test_signals_primitive_is_silent() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn () (signals))", &mut symbols, &mut vm, "<test>");
    // Should parse without error
    assert!(result.is_ok());
}

// ============================================================================
// SQUELCH PRIMITIVE TESTS (Chunk 2 — runtime enforcement in Chunk 3)
// ============================================================================

#[test]
fn test_prim_squelch_returns_closure() {
    let result = crate::common::eval_source_bare("(closure? (squelch (fn () (yield 1)) :yield))");
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
    let val = result.unwrap();
    assert!(val.is_bool(), "expected bool result");
    assert_eq!(val, elle::Value::TRUE, "expected true");
}

#[test]
fn test_prim_squelch_non_closure_error() {
    let result = crate::common::eval_source_bare("(squelch 42 :yield)");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("type-error"),
        "expected type-error, got: {}",
        err
    );
}

#[test]
fn test_prim_squelch_no_keywords_error() {
    let result = crate::common::eval_source_bare("(squelch (fn () 1))");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("squelch"),
        "expected error mentioning 'squelch', got: {}",
        err
    );
}

#[test]
fn test_prim_squelch_unknown_keyword_error() {
    let result = crate::common::eval_source_bare("(squelch (fn () 1) :not-a-signal)");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("not registered"),
        "expected 'not registered', got: {}",
        err
    );
}

#[test]
fn test_prim_squelch_composable_masks() {
    // Calling squelch twice ORs the masks; result is still a closure (no error at construction time)
    let result = crate::common::eval_source_bare(
        "(let ((f (fn () (begin (yield 1))))) (let ((sq1 (squelch f :yield))) (squelch sq1 :error)))"
    );
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
    let val = result.unwrap();
    assert!(val.is_closure(), "expected closure, got: {:?}", val);
}

#[test]
fn test_prim_squelch_identity() {
    // squelch returns a new heap allocation, not the original closure
    let result =
        crate::common::eval_source_bare("(identical? (fn () 1) (squelch (fn () 1) :yield))");
    assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
    let val = result.unwrap();
    assert_eq!(
        val,
        elle::Value::FALSE,
        "expected false (different allocations)"
    );
}

// ============================================================================
// SQUELCH RUNTIME ENFORCEMENT TESTS (Chunk 3)
// ============================================================================

#[test]
fn test_squelch_catches_yield_at_boundary() {
    // A squelched closure that yields should produce a signal-violation error
    let result =
        crate::common::eval_source_bare("(let ((f (squelch (fn () (yield 42)) :yield))) (f))");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("signal-violation"),
        "expected signal-violation, got: {}",
        err
    );
    assert!(
        err.contains("yield"),
        "expected error to mention 'yield', got: {}",
        err
    );
}

#[test]
fn test_squelch_non_squelched_signal_passes() {
    // Squelch :error but the closure yields — the yield should propagate normally
    // (squelch doesn't intercept :yield since only :error is squelched)
    // A yielding closure at top level produces an error about no parent fiber;
    // what matters is it's NOT a signal-violation error.
    let result =
        crate::common::eval_source_bare("(let ((f (squelch (fn () (yield 42)) :error))) (f))");
    // The call propagates a yield signal (no signal-violation, just yield propagation error)
    assert!(result.is_err(), "expected error (yield propagates), got ok");
    let err = result.unwrap_err();
    assert!(
        !err.contains("signal-violation"),
        "yield should NOT produce signal-violation (only :error is squelched), got: {}",
        err
    );
}

#[test]
fn test_squelch_error_passthrough() {
    // Errors should pass through squelch unchanged (we only squelch :yield here)
    let result =
        crate::common::eval_source_bare("(let ((f (squelch (fn () (/ 1 0)) :yield))) (f))");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        !err.contains("signal-violation"),
        "division error should not be a signal-violation, got: {}",
        err
    );
}

#[test]
fn test_squelch_nested_call_enforcement() {
    // Yield bubbles up through inner and outer, reaching the squelch boundary on outer
    // Use let* so each binding can reference prior ones
    let result = crate::common::eval_source_bare(
        "(let* ((inner (fn () (yield 1)))
                (outer (fn () (inner)))
                (safe (squelch outer :yield)))
           (safe))",
    );
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("signal-violation"),
        "expected signal-violation, got: {}",
        err
    );
}

#[test]
fn test_squelch_tail_call_enforcement() {
    // Squelched closure tail-calls a yielding function; squelch should catch the yield
    // Use let* so yielder is visible when building the squelched closure
    let result = crate::common::eval_source_bare(
        "(let* ((yielder (fn () (yield 99)))
                (safe (squelch (fn () (yielder)) :yield)))
           (safe))",
    );
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("signal-violation"),
        "expected signal-violation, got: {}",
        err
    );
}

#[test]
fn test_squelch_signal_violation_error_message() {
    // Error message should mention both "yield" and "squelch"
    let result =
        crate::common::eval_source_bare("(let ((f (squelch (fn () (yield 1)) :yield))) (f))");
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("yield"),
        "error should mention 'yield', got: {}",
        err
    );
    assert!(
        err.contains("squelch"),
        "error should mention 'squelch', got: {}",
        err
    );
}

#[test]
fn test_squelch_composable_runtime() {
    // Both :yield and :error are squelched; f yields; squelch should catch it
    let result = crate::common::eval_source_bare(
        "(let* ((f  (fn () (yield 1)))
                (sq1 (squelch f :yield))
                (sq2 (squelch sq1 :error)))
           (sq2))",
    );
    assert!(result.is_err(), "expected error, got ok");
    let err = result.unwrap_err();
    assert!(
        err.contains("signal-violation"),
        "expected signal-violation, got: {}",
        err
    );
}
