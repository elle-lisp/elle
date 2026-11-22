// audited: 2026-11-22
// The signal of a primitive or intrinsic call, of a lambda body, and of a call through an unknown callee.
//
// docs/signals/inference.md

use super::*;

// ============================================================================
// PRIMITIVE SIGNAL TESTS
// ============================================================================

#[test]
fn test_signal_of_primitive_and_intrinsic_calls() {
    let (mut symbols, mut vm) = setup();

    let errors_calls = [
        "(first (list 1 2))",
        "(rest (list 1 2))",
        "(length (list 1 2 3))",
    ];

    for call in errors_calls {
        let result = analyze_with_stdlib(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::errors(),
            "Primitive call '{}' should have errors signal (type/arity checks)",
            call
        );
    }

    // `number?` and `string?` are stdlib functions, each a total `match` over
    // `%type-of`. Their arity is known, so a call to one cannot raise. The
    // stdlib resolves them to their definitions rather than as unknown globals.
    for call in ["(number? 42)", "(string? \"hello\")"] {
        let result = analyze_with_stdlib(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::silent(),
            "Stdlib predicate call '{}' should be silent",
            call
        );
    }

    // %-intrinsics are silent (no type/arity error path)
    let silent_intrinsic_calls = [
        "(%add 1 2)",
        "(%sub 5 3)",
        "(%mul 2 3)",
        "(%div 10 2)",
        "(%lt 1 2)",
        "(%gt 2 1)",
        "(%eq 1 1)",
        "(%pair 1 2)",
        "(%not true)",
    ];

    for call in silent_intrinsic_calls {
        let result = analyze(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::silent(),
            "Intrinsic call '{}' should be silent",
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
// LAMBDA BODY SIGNAL TRACKING
// ============================================================================

#[test]
fn test_lambda_body_signal_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (yield x))", &mut symbols, &mut vm, "<test>").unwrap();

    if let Some(lambda) = result.hir.kind.as_lambda() {
        assert_eq!(lambda.body.signal, Signal::yields());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_signal_nested_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(fn (x) (begin (%add x 1) (yield x) (%add x 2)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let Some(lambda) = result.hir.kind.as_lambda() {
        assert_eq!(lambda.body.signal, Signal::yields());
    } else {
        panic!("Expected Lambda");
    }
}

// ============================================================================
// UNKNOWN CALLEE SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_signal_parameter_call_yields_errors() {
    // Calling a function parameter: yields_errors() (may yield + inherent error)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (f) (f 42))", &mut symbols, &mut vm, "<test>").unwrap();
    if let Some(lambda) = result.hir.kind.as_lambda() {
        assert_eq!(
            lambda.body.signal,
            Signal::yields_errors(),
            "Calling a function parameter has yields_errors() signal"
        );
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_signal_let_bound_non_lambda_call_is_unknown() {
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
