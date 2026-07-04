use super::*;

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
            // Purely polymorphic on param 0 — no inherent error from parameter calls
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
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (def apply-fn (fn (f x) (f x))) (apply-fn length 42))",
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
        Signal::yields(),
        "Calling polymorphic function with yielding arg should be Yields"
    );
}

#[test]
fn test_polymorphic_inference_my_map() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def my-map (fn (f xs)              (if (empty? xs) (list)                  (%pair (f (first xs)) (my-map f (rest xs))))))           (my-map length (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Recursive higher-order function with errors arg should have errors signal"
    );
}

// KNOWN BUG (deferred): within-file signal inference does not converge for
// *mutually* recursive functions. The spec (docs/signals/inference.md, "Mutual
// Recursion Across Files") states the fixpoint converges for mutually recursive
// definitions within a single file, yet the analyzer under-approximates them.
//
// `(letrec [foo<->bar] (foo 3))` where `bar` calls `first` (an errors
// primitive) infers `silent` for the whole expression instead of `errors`; the
// closure-template path (file-letrec → lowering, e.g. the nqueens
// `try-cols-helper`/`solve-helper` pair) infers `unknown()`. A SELF-recursive
// function with the same `first` call converges correctly to `errors`, so the
// gap is specifically the mutual cycle.
//
// Root cause (partial): the final fixpoint pass in `src/hir/analyze/fileletrec/
// letrec.rs` re-analyzes only lambda bindings; bare expression entries keep
// their stale callee signals, and the file-letrec/lowering path leaks
// `unknown()` for the mutual pair. See docs/signals/inference.md for the
// intended fixpoint; the related closure-template assertions live in
// `pipeline::test_nqueens_functions_are_pure` and
// `jit::test_nqueens_eval_signals_are_silent`.
#[test]
#[ignore = "known bug: within-file mutual-recursion signal fixpoint does not converge"]
fn test_mutual_recursion_signal_converges() {
    let (mut symbols, mut vm) = setup();
    // foo <-> bar; bar calls `first` (errors). Both must converge to errors.
    let result = analyze(
        "(letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1)))) \
                  bar (fn (n) (if (%eq n 0) (first (list 1)) (foo (%sub n 1))))] \
           (foo 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "mutually recursive foo/bar (bar calls `first`) must converge to errors"
    );

    // Control: the self-recursive analogue already converges correctly. Kept in
    // the same test so a fix that regresses self-recursion is caught too.
    let selfrec = analyze(
        "(letrec [foo (fn (n) (if (%eq n 0) (first (list 1)) (foo (%sub n 1))))] (foo 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        selfrec.hir.signal,
        Signal::errors(),
        "self-recursive control must be errors"
    );
}

#[test]
fn test_polymorphic_inference_non_recursive_map() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(        r#"(begin            (def apply-to-list (fn (f xs)              (if (empty? xs) (list)                  (%pair (f (first xs)) (list)))))           (apply-to-list length (list 1 2 3)))"#,        &mut symbols, &mut vm, "<test>")
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "Non-recursive higher-order function with errors arg should have errors signal"
    );
}

#[test]
fn test_polymorphic_inference_direct_yield_prevents() {
    // A function that both calls a parameter AND yields directly:
    // inherent SIG_YIELD from (yield 99), polymorphic on param 0 from (f x).
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
                Signal {
                    bits: SIG_YIELD,
                    propagates: 1,
                },
                "Function with direct yield + param call should be Yields + Polymorphic(0)"
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
            // Purely polymorphic on params 0 and 1 — no inherent error
            assert_eq!(
                *inferred_signals,
                Signal {
                    bits: SIG_OK,
                    propagates: 0b11,
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
    let (mut symbols, mut vm) = setup();
    let result = analyze_with_stdlib(
        "(begin (def apply-both (fn (f g x) (begin (f x) (g x)))) (apply-both length number? 5))",
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
    let result = analyze_with_stdlib(        r#"(begin            (def gen (fn () (yield 1)))           (def apply-both (fn (f g x) (begin (f x) (g x))))            (apply-both gen number? 5))"#,        &mut symbols, &mut vm, "<test>")
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
fn test_polymorphic_inference_with_known_yielding_call() {
    // Function that calls a parameter AND a known yielding function.
    // Inherent SIG_YIELD from calling gen, polymorphic on param 0 from (f x).
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
                    Signal {
                        bits: SIG_YIELD,
                        propagates: 1,
                    },
                    "Function with known yielding call + param call should be Yields + Polymorphic(0)"
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
