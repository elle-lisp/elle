use super::*;

// ============================================================================
// 7. PRIMITIVE EFFECT TESTS
// ============================================================================

#[test]
fn test_signal_pure_primitives() {
    let (mut symbols, mut vm) = setup();

    let errors_calls = [
        "(first (list 1 2))",
        "(rest (list 1 2))",
        "(length (list 1 2 3))",
        "(number? 42)",
        "(string? \"hello\")",
    ];

    for call in errors_calls {
        // `number?`/`string?` are stdlib functions (defined via `type-of`, which
        // carries `errors`); analyze with stdlib so they resolve to their
        // definitions rather than as unknown globals.
        let result = analyze_with_stdlib(call, &mut symbols, &mut vm, "<test>").unwrap();
        assert_eq!(
            result.hir.signal,
            Signal::errors(),
            "Primitive call '{}' should have errors signal (type/arity checks)",
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
        "(fn (x) (begin (%add x 1) (yield x) (%add x 2)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.signal, Signal::yields());
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
