// Integration tests for JIT-compiled yielding functions.
//
// These tests verify that yield propagates correctly through JIT-compiled
// call chains. The JIT side-exits to the interpreter on yield; resume
// goes through the interpreter.
//
// Test strategy: We define a hot outer function that calls a yielding
// inner function. The outer function is called enough times (via warm-up)
// to trigger JIT compilation. When the inner function yields, the yield
// propagates through the JIT-compiled outer function's call instruction.
//
// Direct coroutine tests (make-coroutine + coro/resume) do NOT exercise
// the JIT path because coroutine execution bypasses call_inner.

use crate::common::eval_source;

#[test]
fn test_jit_yield_through_call() {
    let source = r#"
        (def inner (fn () (yield 42) 99))
        (def outer (fn () (inner)))
        (def run (fn () (outer)))

        # Warm up: run calls outer via call_inner (profiling outer),
        # outer calls inner via call_inner (profiling inner).
        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (set warmup-i (+ warmup-i 1)))

        # Now outer should be JIT-compiled. Test it.
        (def c (make-coroutine run))
        (def v1 (coro/resume c))
        (def v2 (coro/resume c))
        (list v1 v2)
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(format!("{}", result), "(42 99)");
}

#[test]
fn test_jit_yield_through_call_with_resume_value() {
    let source = r#"
        (def inner (fn () (def x (yield 1)) (+ x 10)))
        (def outer (fn () (inner)))
        (def run (fn () (outer)))

        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c 0)
          (set warmup-i (+ warmup-i 1)))

        (def c (make-coroutine run))
        (coro/resume c)
        (coro/resume c 5)
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_jit_yield_through_call_multiple_yields() {
    let source = r#"
        (def inner (fn () (yield 1) (yield 2) (yield 3) 4))
        (def outer (fn () (inner)))
        (def run (fn () (outer)))

        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (set warmup-i (+ warmup-i 1)))

        (def c (make-coroutine run))
        (list (coro/resume c) (coro/resume c) (coro/resume c) (coro/resume c))
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(format!("{}", result), "(1 2 3 4)");
}

#[test]
fn test_jit_yield_through_call_stack_preservation() {
    // Tests that values computed before the call survive yield.
    // outer computes (+ 1 (inner)), inner yields 10 then returns 20.
    // First resume yields 10, second resume returns 1 + 20 = 21.
    //
    // Note: inner avoids local variables (def x ...) because the JIT
    // yield helper uses closure.env (captures only), not the full env
    // that build_closure_env creates. Local variables in the yielding
    // function would cause StoreUpvalue to fail on resume. This is a
    // known limitation tracked separately.
    let source = r#"
        (def inner (fn () (yield 10) 20))
        (def outer (fn () (+ 1 (inner))))
        (def run (fn () (outer)))

        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (set warmup-i (+ warmup-i 1)))

        (def c (make-coroutine run))
        (def v1 (coro/resume c))
        (def v2 (coro/resume c))
        (list v1 v2)
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(format!("{}", result), "(10 21)");
}

#[test]
fn test_jit_yield_through_nested_calls() {
    // Three levels: outer -> middle -> inner. inner yields.
    // All three should be JIT-compiled after warm-up.
    let source = r#"
        (def inner (fn () (yield 42) 99))
        (def middle (fn () (inner)))
        (def outer (fn () (middle)))
        (def run (fn () (outer)))

        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (set warmup-i (+ warmup-i 1)))

        (def c (make-coroutine run))
        (def v1 (coro/resume c))
        (def v2 (coro/resume c))
        (list v1 v2)
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(format!("{}", result), "(42 99)");
}

#[test]
fn test_jit_yield_with_captures() {
    // Outer captures a variable and passes it to inner.
    let source = r#"
        (def make-gen (fn (offset)
          (fn () (yield offset) (+ offset 1))))
        (def outer (fn () ((make-gen 10))))
        (def run (fn () (outer)))

        (var warmup-i 0)
        (forever
          (if (>= warmup-i 15) (break))
          (def warmup-c (make-coroutine run))
          (coro/resume warmup-c)
          (coro/resume warmup-c)
          (set warmup-i (+ warmup-i 1)))

        (def c (make-coroutine run))
        (def v1 (coro/resume c))
        (def v2 (coro/resume c))
        (list v1 v2)
    "#;
    let result = eval_source(source).unwrap();
    assert_eq!(format!("{}", result), "(10 11)");
}
