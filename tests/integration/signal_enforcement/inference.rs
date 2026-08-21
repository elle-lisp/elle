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

// A mutual cycle must converge to the same signal a self-recursive function
// with the same body converges to. The seed is optimistic (`Signal::silent()`),
// so a loop that stops before the cycle settles under-approximates — and an
// under-approximated signal is the direction that costs a guarantee: a function
// that can raise `:error` but reads as silent satisfies a compile-time
// `(silence)` check and aborts at runtime instead. `docs/pipeline.md`
// § The fixpoint loop states the rule; `silence_survives_a_mutual_cycle` below
// pins the consequence end to end.
#[test]
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

/// Direction matters: the cycle must converge whichever order its members are
/// written in. `test_mutual_recursion_signal_converges` calls the member that
/// is *not* the one raising; this calls the raiser first, so a loop that
/// happens to settle only because the last binding analyzed was the raising one
/// still fails here.
#[test]
fn test_mutual_recursion_converges_from_either_entry() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(letrec [foo (fn (n) (if (%eq n 0) (first (list 1)) (bar (%sub n 1)))) \
                  bar (fn (n) (if (%eq n 0) 0 (foo (%sub n 1))))] \
           (bar 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "entering the cycle at the non-raising member must still see errors"
    );
}

/// A three-way cycle: the raiser is two edges away from the entry point, so one
/// re-analysis pass is not enough to carry the signal back around.
#[test]
fn test_three_way_mutual_recursion_converges() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(letrec [a (fn (n) (if (%eq n 0) 0 (b (%sub n 1)))) \
                  b (fn (n) (if (%eq n 0) 0 (c (%sub n 1)))) \
                  c (fn (n) (if (%eq n 0) (first (list 1)) (a (%sub n 1))))] \
           (a 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::errors(),
        "a three-way cycle must propagate the raiser's signal all the way round"
    );
}

/// Convergence must not invent signals either. A mutual cycle whose members all
/// stay silent has to remain silent — an over-approximation would reject
/// correct `(silence)` code, which is the opposite failure and just as wrong.
#[test]
fn test_silent_mutual_recursion_stays_silent() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1)))) \
                  bar (fn (n) (if (%eq n 0) 1 (foo (%sub n 1))))] \
           (foo 3))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    assert_eq!(
        result.hir.signal,
        Signal::silent(),
        "a silent mutual cycle must not be inflated to a signalling one"
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
