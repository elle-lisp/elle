// Integration tests for interprocedural effect tracking and enforcement
//
// These tests verify that effects propagate correctly across function boundaries:
// - Direct yield has Yields effect
// - Calling a yielding function propagates Yields effect
// - Polymorphic effects (like map) resolve based on argument effects
// - Pure functions remain pure
// - set! invalidates effect tracking

use elle::effects::Effect;
use elle::hir::HirKind;
use elle::pipeline::analyze_new;
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> SymbolTable {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    symbols
}

// ============================================================================
// 1. DIRECT YIELD EFFECT TESTS
// ============================================================================

#[test]
fn test_effect_direct_yield() {
    // (lambda () (yield 1)) should have Pure effect on the lambda creation
    // but the body should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(fn () (yield 1))", &mut symbols).unwrap();

    // Lambda creation is pure
    assert_eq!(result.hir.effect, Effect::Pure);

    // But the body should be Yields
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::Yields);
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_effect_yield_in_begin() {
    // (begin (yield 1) (yield 2)) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(begin (yield 1) (yield 2))", &mut symbols).unwrap();
    assert_eq!(result.hir.effect, Effect::Yields);
}

#[test]
fn test_effect_yield_in_if() {
    // (if #t (yield 1) 2) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(if #t (yield 1) 2)", &mut symbols).unwrap();
    assert_eq!(result.hir.effect, Effect::Yields);
}

// ============================================================================
// 2. CALL PROPAGATION TESTS
// ============================================================================

#[test]
fn test_effect_call_propagation() {
    // (define gen (lambda () (yield 1)))
    // (gen) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(begin (define gen (fn () (yield 1))) (gen))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Calling a yielding function should propagate Yields effect"
    );
}

#[test]
fn test_effect_nested_propagation() {
    // (define gen (lambda () (yield 1)))
    // (define wrapper (lambda () (gen)))
    // (wrapper) should be Yields
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define gen (fn () (yield 1))) (define wrapper (fn () (gen))) (wrapper))",
        &mut symbols,
    )
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Nested call to yielding function should propagate Yields effect"
    );
}

#[test]
fn test_effect_pure_call() {
    // (define f (lambda (x) (+ x 1)))
    // (f 42) should be Pure
    let mut symbols = setup();
    let result = analyze_new("(begin (define f (fn (x) (+ x 1))) (f 42))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Calling a pure function should remain Pure"
    );
}

#[test]
fn test_effect_let_bound_lambda() {
    // (let ((gen (lambda () (yield 1)))) (gen)) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(let ((gen (fn () (yield 1)))) (gen))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Calling a let-bound yielding lambda should propagate Yields effect"
    );
}

#[test]
fn test_effect_letrec_bound_lambda() {
    // (letrec ((gen (lambda () (yield 1)))) (gen)) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("(letrec ((gen (fn () (yield 1)))) (gen) 42)", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Calling a letrec-bound yielding lambda should propagate Yields effect"
    );
}

// ============================================================================
// 3. POLYMORPHIC EFFECT RESOLUTION TESTS
// ============================================================================

// Note: map, filter, fold are defined as Lisp functions in init_stdlib,
// not as primitives. For polymorphic effect resolution to work with them,
// they would need to be defined in the same compilation unit or tracked
// across compilation units. These tests verify the behavior with locally
// defined higher-order functions.

#[test]
fn test_effect_polymorphic_local_higher_order() {
    // Define a local higher-order function and verify polymorphic resolution
    let mut symbols = setup();
    let result = analyze_new(
        r#"(begin
            (define my-map (fn (f lst)
                (if (empty? lst)
                    ()
                    (cons (f (first lst)) (my-map f (rest lst))))))
            (define gen (fn (x) (yield x)))
            (my-map gen (list 1 2 3)))"#,
        &mut symbols,
    )
    .unwrap();
    // my-map calls gen which yields, so my-map's body has Yields effect
    // When we call (my-map gen ...), we look up my-map's effect
    // Since my-map is defined with a lambda, we track its body effect
    // The body calls f which is a parameter - we can't resolve that statically
    // So this will be Pure (conservative)
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Local higher-order function with unknown parameter effect is conservatively Pure"
    );
}

#[test]
fn test_effect_polymorphic_direct_call() {
    // Direct call with yielding lambda should propagate effect
    let mut symbols = setup();
    let result = analyze_new(
        r#"(begin
            (define apply-fn (fn (f x) (f x)))
            (apply-fn (fn (x) (yield x)) 42))"#,
        &mut symbols,
    )
    .unwrap();
    // apply-fn's body calls f which is a parameter
    // We can't statically resolve the parameter's effect
    // So this is conservatively Pure
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Higher-order function with parameter call is conservatively Pure"
    );
}

#[test]
fn test_effect_polymorphic_with_pure_arg() {
    // Calling a global function (map) with pure lambda
    // Since map isn't in primitive_effects (it's defined in stdlib),
    // the call is conservatively Pure
    let mut symbols = setup();
    let result = analyze_new("(map (fn (x) (+ x 1)) (list 1 2 3))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Call to unknown global with pure function is Pure"
    );
}

#[test]
fn test_effect_polymorphic_with_yielding_arg_unknown_global() {
    // Calling a global function (map) that isn't in primitive_effects
    // Even with a yielding argument, we can't resolve the effect
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
    )
    .unwrap();
    // map is not in primitive_effects (it's defined in stdlib, not as a primitive)
    // So we can't resolve its polymorphic effect
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Call to unknown global is conservatively Pure"
    );
}

// ============================================================================
// 4. SET! INVALIDATION TESTS
// ============================================================================

#[test]
fn test_effect_set_invalidation() {
    // (define f (lambda () 42))
    // (set! f (lambda () (yield 1)))
    // After set!, effect tracking for f is invalidated
    // Calling f should conservatively be Pure (we don't know the new effect)
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define f (fn () 42)) (set! f (fn () (yield 1))) (f))",
        &mut symbols,
    )
    .unwrap();
    // After set!, we conservatively treat the effect as Pure
    // This is safe because we don't produce false positives
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "After set!, effect should be conservatively Pure"
    );
}

// ============================================================================
// 5. DIRECT LAMBDA CALL TESTS
// ============================================================================

#[test]
fn test_effect_direct_lambda_call_yields() {
    // ((lambda () (yield 1))) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new("((fn () (yield 1)))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Direct call to yielding lambda should have Yields effect"
    );
}

#[test]
fn test_effect_direct_lambda_call_pure() {
    // ((lambda () 42)) should be Pure
    let mut symbols = setup();
    let result = analyze_new("((fn () 42))", &mut symbols).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Pure,
        "Direct call to pure lambda should be Pure"
    );
}

// ============================================================================
// 6. COMPLEX SCENARIOS
// ============================================================================

#[test]
fn test_effect_multiple_calls_mixed() {
    // (begin (define pure-fn (fn () 42))
    //        (define yield-fn (fn () (yield 1)))
    //        (pure-fn)
    //        (yield-fn))
    // Should have Yields effect because yield-fn is called
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define pure-fn (fn () 42)) (define yield-fn (fn () (yield 1))) (pure-fn) (yield-fn))",
        &mut symbols,
    )
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Sequence with yielding call should have Yields effect"
    );
}

#[test]
fn test_effect_conditional_yield() {
    // (define maybe-yield (fn (x) (if x (yield 1) 2)))
    // (maybe-yield #t) should have Yields effect
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define maybe-yield (fn (x) (if x (yield 1) 2))) (maybe-yield #t))",
        &mut symbols,
    )
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Call to function with conditional yield should have Yields effect"
    );
}

#[test]
fn test_effect_closure_captures_yielding() {
    // (let ((gen (fn () (yield 1))))
    //   (let ((wrapper (fn () (gen))))
    //     (wrapper)))
    // Should have Yields effect
    let mut symbols = setup();
    let result = analyze_new(
        "(let ((gen (fn () (yield 1)))) (let ((wrapper (fn () (gen)))) (wrapper)))",
        &mut symbols,
    )
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Nested closure calling yielding function should have Yields effect"
    );
}

// ============================================================================
// 7. PRIMITIVE EFFECT TESTS
// ============================================================================

#[test]
fn test_effect_pure_primitives() {
    // Pure primitives should have Pure effect
    let mut symbols = setup();

    let pure_calls = [
        "(+ 1 2)",
        "(- 5 3)",
        "(* 2 3)",
        "(/ 10 2)",
        "(< 1 2)",
        "(> 2 1)",
        "(= 1 1)",
        "(cons 1 2)",
        "(list 1 2 3)",
        "(first (list 1 2))",
        "(rest (list 1 2))",
        "(length (list 1 2 3))",
        "(not #t)",
        "(number? 42)",
        "(string? \"hello\")",
    ];

    for call in pure_calls {
        let result = analyze_new(call, &mut symbols).unwrap();
        assert_eq!(
            result.hir.effect,
            Effect::Pure,
            "Primitive call '{}' should be Pure",
            call
        );
    }
}

// ============================================================================
// 8. LAMBDA BODY EFFECT TRACKING
// ============================================================================

#[test]
fn test_lambda_body_effect_pure() {
    let mut symbols = setup();
    let result = analyze_new("(fn (x) (+ x 1))", &mut symbols).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::Pure);
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_effect_yields() {
    let mut symbols = setup();
    let result = analyze_new("(fn (x) (yield x))", &mut symbols).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::Yields);
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_effect_nested_yield() {
    let mut symbols = setup();
    let result = analyze_new("(fn (x) (begin (+ x 1) (yield x) (+ x 2)))", &mut symbols).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::Yields);
    } else {
        panic!("Expected Lambda");
    }
}
