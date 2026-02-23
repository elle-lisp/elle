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
use elle::pipeline::{analyze, analyze_all};
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// ============================================================================
// 1. DIRECT YIELD EFFECT TESTS
// ============================================================================

#[test]
fn test_effect_direct_yield() {
    // (fn () (yield 1)) should have Pure effect on the lambda creation
    // but the body should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn () (yield 1))", &mut symbols, &mut vm).unwrap();

    // Lambda creation is pure
    assert_eq!(result.hir.effect, Effect::none());

    // But the body should be Yields
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::yields());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_effect_yield_in_begin() {
    // (begin (yield 1) (yield 2)) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(begin (yield 1) (yield 2))", &mut symbols, &mut vm).unwrap();
    assert_eq!(result.hir.effect, Effect::yields());
}

#[test]
fn test_effect_yield_in_if() {
    // (if #t (yield 1) 2) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(if #t (yield 1) 2)", &mut symbols, &mut vm).unwrap();
    assert_eq!(result.hir.effect, Effect::yields());
}

// ============================================================================
// 2. CALL PROPAGATION TESTS
// ============================================================================

#[test]
fn test_effect_call_propagation() {
    // (define gen (fn () (yield 1)))
    // (gen) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(begin (define gen (fn () (yield 1))) (gen))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Calling a yielding function should propagate Yields effect"
    );
}

#[test]
fn test_effect_nested_propagation() {
    // (define gen (fn () (yield 1)))
    // (define wrapper (fn () (gen)))
    // (wrapper) should be Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define gen (fn () (yield 1))) (define wrapper (fn () (gen))) (wrapper))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Nested call to yielding function should propagate Yields effect"
    );
}

#[test]
fn test_effect_pure_call() {
    // (define f (fn (x) (+ x 1)))
    // (f 42) should be Pure
    let (mut symbols, mut vm) = setup();
    let result = analyze("(begin (define f (fn (x) (+ x 1))) (f 42))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Calling a pure function should remain Pure"
    );
}

#[test]
fn test_effect_let_bound_lambda() {
    // (let ((gen (fn () (yield 1)))) (gen)) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(let ((gen (fn () (yield 1)))) (gen))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Calling a let-bound yielding lambda should propagate Yields effect"
    );
}

#[test]
fn test_effect_letrec_bound_lambda() {
    // (letrec ((gen (fn () (yield 1)))) (gen)) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("(letrec ((gen (fn () (yield 1)))) (gen) 42)", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
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
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define my-map (fn (f lst)                (if (empty? lst)                    ()                    (cons (f (first lst)) (my-map f (rest lst))))))            (define gen (fn (x) (yield x)))            (my-map gen (list 1 2 3)))"#,        &mut symbols, &mut vm)
    .unwrap();
    // my-map calls gen which yields, so my-map's body has Yields effect
    // When we call (my-map gen ...), we look up my-map's effect
    // Since my-map is defined with a lambda, we track its body effect
    // The body calls f which is a parameter - we can't resolve that statically
    // So this is Yields (sound: unknown callee may yield)
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Local higher-order function with unknown parameter effect is conservatively Yields"
    );
}

#[test]
fn test_effect_polymorphic_direct_call() {
    // Direct call with yielding lambda should propagate effect
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define apply-fn (fn (f x) (f x)))            (apply-fn (fn (x) (yield x)) 42))"#,        &mut symbols, &mut vm)
    .unwrap();
    // apply-fn's body calls f which is a parameter
    // We can't statically resolve the parameter's effect
    // So this is Yields (sound: unknown callee may yield)
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Higher-order function with parameter call is conservatively Yields"
    );
}

#[test]
fn test_effect_polymorphic_with_pure_arg() {
    // Calling a global function (map) with pure lambda
    // Since map isn't in primitive_effects (it's defined in stdlib),
    // the call is conservatively Yields (sound: unknown global may yield)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(map (fn (x) (+ x 1)) (list 1 2 3))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Call to unknown global is Yields (sound default)"
    );
}

#[test]
fn test_effect_polymorphic_with_yielding_arg_unknown_global() {
    // Calling a global function (map) that isn't in primitive_effects
    // Unknown globals default to Yields for soundness
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define gen (fn (x) (yield x))) (map gen (list 1 2 3)))",        &mut symbols, &mut vm)
    .unwrap();
    // map is not in primitive_effects (it's defined in stdlib, not as a primitive)
    // Unknown globals are Yields for soundness
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Call to unknown global is Yields (sound default)"
    );
}

// ============================================================================
// 4. SET! INVALIDATION TESTS
// ============================================================================

#[test]
fn test_effect_set_invalidation() {
    // (define f (fn () 42))
    // (set! f (fn () (yield 1)))
    // After set!, effect tracking for f is invalidated
    // Calling f should be Yields (sound: we can't prove it's pure)
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define f (fn () 42)) (set! f (fn () (yield 1))) (f))",        &mut symbols, &mut vm)
    .unwrap();
    // After set!, we conservatively treat the effect as Yields
    // This is sound: we can't prove the new value is pure
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "After set!, effect should be Yields (sound default)"
    );
}

// ============================================================================
// 5. DIRECT LAMBDA CALL TESTS
// ============================================================================

#[test]
fn test_effect_direct_lambda_call_yields() {
    // ((fn () (yield 1))) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze("((fn () (yield 1)))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Direct call to yielding lambda should have Yields effect"
    );
}

#[test]
fn test_effect_direct_lambda_call_pure() {
    // ((fn () 42)) should be Pure
    let (mut symbols, mut vm) = setup();
    let result = analyze("((fn () 42))", &mut symbols, &mut vm).unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::none(),
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
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define pure-fn (fn () 42)) (define yield-fn (fn () (yield 1))) (pure-fn) (yield-fn))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Sequence with yielding call should have Yields effect"
    );
}

#[test]
fn test_effect_conditional_yield() {
    // (define maybe-yield (fn (x) (if x (yield 1) 2)))
    // (maybe-yield #t) should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define maybe-yield (fn (x) (if x (yield 1) 2))) (maybe-yield #t))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Call to function with conditional yield should have Yields effect"
    );
}

#[test]
fn test_effect_closure_captures_yielding() {
    // (let ((gen (fn () (yield 1))))
    //   (let ((wrapper (fn () (gen))))
    //     (wrapper)))
    // Should have Yields effect
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(let ((gen (fn () (yield 1)))) (let ((wrapper (fn () (gen)))) (wrapper)))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Nested closure calling yielding function should have Yields effect"
    );
}

// ============================================================================
// 7. PRIMITIVE EFFECT TESTS
// ============================================================================

#[test]
fn test_effect_pure_primitives() {
    // Pure primitives should have Pure effect
    let (mut symbols, mut vm) = setup();

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
        let result = analyze(call, &mut symbols, &mut vm).unwrap();
        assert_eq!(
            result.hir.effect,
            Effect::none(),
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
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (+ x 1))", &mut symbols, &mut vm).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::none());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_effect_yields() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (yield x))", &mut symbols, &mut vm).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::yields());
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_lambda_body_effect_nested_yield() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) (begin (+ x 1) (yield x) (+ x 2)))", &mut symbols, &mut vm).unwrap();

    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(body.effect, Effect::yields());
    } else {
        panic!("Expected Lambda");
    }
}

// ============================================================================
// 9. UNKNOWN GLOBAL SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_effect_unknown_global_is_yields() {
    // Unknown global functions default to Yields (sound)
    // This is the fix for effect soundness: if we can't prove a global is pure,
    // we must assume it may yield (since it could be redefined via set!)
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define f (fn () 42)) (set! f (fn () (yield 1))) (f))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Unknown global should be Yields for soundness"
    );
}

// ============================================================================
// 10. UNKNOWN CALLEE SOUNDNESS TESTS
// ============================================================================

#[test]
fn test_effect_parameter_call_is_yields() {
    // Calling a function parameter should be Yields (we can't know its effect)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (f) (f 42))", &mut symbols, &mut vm).unwrap();
    if let HirKind::Lambda { body, .. } = &result.hir.kind {
        assert_eq!(
            body.effect,
            Effect::yields(),
            "Calling a function parameter should be Yields (unknown effect)"
        );
    } else {
        panic!("Expected Lambda");
    }
}

#[test]
fn test_effect_let_bound_non_lambda_call_is_yields() {
    // Calling a let-bound non-lambda should be Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze("(let ((f (first fns))) (f 42))", &mut symbols, &mut vm).unwrap();
    // f is not a lambda literal, effect unknown → Yields
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Calling a let-bound non-lambda should be Yields (unknown effect)"
    );
}

// ============================================================================
// 11. AUTOMATIC POLYMORPHIC EFFECT INFERENCE TESTS
// ============================================================================

#[test]
fn test_polymorphic_inference_single_param() {
    // Higher-order function should infer Polymorphic(0)
    let (mut symbols, mut vm) = setup();
    let result = analyze("(define apply-fn (fn (f x) (f x)))", &mut symbols, &mut vm).unwrap();

    // Check the lambda's inferred effect
    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::polymorphic(0),
                "apply-fn should have Polymorphic(0) effect"
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
    let result = analyze(        "(begin (define apply-fn (fn (f x) (f x))) (apply-fn + 42))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Calling polymorphic function with pure arg should be Pure"
    );
}

#[test]
fn test_polymorphic_inference_resolves_yields() {
    // Calling apply-fn with a yielding lambda should be Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(begin (define apply-fn (fn (f x) (f x))) (apply-fn (fn (x) (yield x)) 42))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Calling polymorphic function with yielding arg should be Yields"
    );
}

#[test]
fn test_polymorphic_inference_my_map() {
    // User-defined recursive map - the recursive call is seeded with Pure
    // during analysis (since define seeds lambda forms with Pure before
    // analyzing the body), so the function correctly infers as Polymorphic(0).
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define my-map (fn (f xs)              (if (empty? xs) (list)                  (cons (f (first xs)) (my-map f (rest xs))))))           (my-map + (list 1 2 3)))"#,        &mut symbols, &mut vm)
    .unwrap();
    // my-map is Polymorphic(0) because the only Yields source is calling f.
    // The recursive call to my-map is seeded as Pure during analysis.
    // When called with +, which is Pure, the result is Pure.
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Recursive higher-order function with pure arg should be Pure"
    );
}

#[test]
fn test_polymorphic_inference_non_recursive_map() {
    // Non-recursive higher-order function should be Polymorphic(0)
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define apply-to-list (fn (f xs)              (if (empty? xs) (list)                  (cons (f (first xs)) (list)))))           (apply-to-list + (list 1 2 3)))"#,        &mut symbols, &mut vm)
    .unwrap();
    // apply-to-list is Polymorphic(0), + is Pure, so the call is Pure
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Non-recursive higher-order function with pure arg should be Pure"
    );
}

#[test]
fn test_polymorphic_inference_direct_yield_prevents() {
    // A function that both calls a parameter AND yields directly is Yields, not Polymorphic
    let (mut symbols, mut vm) = setup();
    let result = analyze(        "(define bad (fn (f x) (begin (yield 99) (f x))))",        &mut symbols, &mut vm)
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::yields(),
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
    let result = analyze(        "(define apply-both (fn (f g x) (begin (f x) (g x))))",        &mut symbols, &mut vm)
    .unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect {
                    bits: 0,
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
    let result = analyze(        "(begin (define apply-both (fn (f g x) (begin (f x) (g x)))) (apply-both + * 5))",        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Calling Polymorphic({{0,1}}) with two pure args should be Pure"
    );
}

#[test]
fn test_polymorphic_inference_two_params_resolves_yields() {
    // Calling apply-both with one yielding function should be Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define gen (fn () (yield 1)))           (define apply-both (fn (f g x) (begin (f x) (g x))))            (apply-both gen * 5))"#,        &mut symbols, &mut vm)
    .unwrap();
    assert_eq!(
        result.hir.effect,
        Effect::yields(),
        "Calling Polymorphic({{0,1}}) with one yielding arg should be Yields"
    );
}

#[test]
fn test_polymorphic_inference_second_param() {
    // Higher-order function where the second parameter is called
    let (mut symbols, mut vm) = setup();
    let result = analyze("(define apply-second (fn (x f) (f x)))", &mut symbols, &mut vm).unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::polymorphic(1),
                "apply-second should have Polymorphic(1) effect"
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
    let result = analyze(        r#"(begin            (define apply-fn (fn (f x) (f x)))           (define wrapper (fn (g y) (apply-fn g y)))           (wrapper + 42))"#,        &mut symbols, &mut vm)
    .unwrap();
    // wrapper calls apply-fn with g, apply-fn is Polymorphic(0)
    // So wrapper's body effect depends on g's effect
    // wrapper should be Polymorphic(0) and the final call with + should be Pure
    assert_eq!(
        result.hir.effect,
        Effect::none(),
        "Nested polymorphic calls with pure arg should be Pure"
    );
}

#[test]
fn test_polymorphic_inference_with_known_yielding_call() {
    // A function that calls a parameter AND a known yielding function is Yields
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (define gen (fn () (yield 1)))           (define bad (fn (f x) (begin (gen) (f x)))))"#,        &mut symbols, &mut vm)
    .unwrap();

    // Find the 'bad' definition
    if let HirKind::Begin(exprs) = &result.hir.kind {
        if let HirKind::Define { value, .. } = &exprs[1].kind {
            if let HirKind::Lambda {
                inferred_effect, ..
            } = &value.kind
            {
                assert_eq!(
                    *inferred_effect,
                    Effect::yields(),
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
    // A pure function should have Pure effect, not Polymorphic
    let (mut symbols, mut vm) = setup();
    let result = analyze("(define add1 (fn (x) (+ x 1)))", &mut symbols, &mut vm).unwrap();

    if let HirKind::Define { value, .. } = &result.hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::none(),
                "Pure function should have Pure effect"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

// ============================================================================
// 8. CROSS-FORM EFFECT TRACKING TESTS
// ============================================================================

#[test]
fn test_cross_form_effect_tracking_pure_helper() {
    // When a pure helper function is defined in one form and called in another,
    // the caller should know the helper is pure.
    let (mut symbols, mut vm) = setup();
    let results = analyze_all(        r#"        (define helper (fn (x) (+ x 1)))        (define caller (fn (y) (helper y)))        "#,        &mut symbols,    &mut vm,    )
    .unwrap();

    // The second form (caller) should have Pure effect because helper is pure
    assert_eq!(results.len(), 2);

    // Check that caller's lambda has Pure effect
    if let HirKind::Define { value, .. } = &results[1].hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::none(),
                "Caller of pure helper should be Pure"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_cross_form_effect_tracking_polymorphic_helper() {
    // When a polymorphic helper is defined in one form and called in another,
    // the caller should correctly resolve the polymorphic effect.
    let (mut symbols, mut vm) = setup();
    let results = analyze_all(        r#"        (define apply-fn (fn (f x) (f x)))        (define use-apply (fn () (apply-fn + 5)))        "#,        &mut symbols,    &mut vm,    )
    .unwrap();

    assert_eq!(results.len(), 2);

    // Check that use-apply's lambda has Pure effect (because + is pure)
    if let HirKind::Define { value, .. } = &results[1].hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::none(),
                "Caller of polymorphic helper with pure arg should be Pure"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}

#[test]
fn test_cross_form_effect_tracking_mutual_recursion() {
    // Test that mutually recursive functions across forms work correctly.
    // safe? calls check-safe-helper, which should be known as pure.
    let (mut symbols, mut vm) = setup();
    let results = analyze_all(        r#"        (define check-safe-helper (fn (x) (= x 0)))        (define safe? (fn (n) (check-safe-helper n)))        "#,        &mut symbols,    &mut vm,    )
    .unwrap();

    assert_eq!(results.len(), 2);

    // Check that safe?'s lambda has Pure effect
    if let HirKind::Define { value, .. } = &results[1].hir.kind {
        if let HirKind::Lambda {
            inferred_effect, ..
        } = &value.kind
        {
            assert_eq!(
                *inferred_effect,
                Effect::none(),
                "safe? calling pure check-safe-helper should be Pure"
            );
        } else {
            panic!("Expected Lambda");
        }
    } else {
        panic!("Expected Define");
    }
}
