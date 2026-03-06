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

#[test]
fn test_jit_yield_locals_survive_yield_resume() {
    // Regression test for #461: local variables lost across JIT yield/resume.
    //
    // The interpreter stores locals on the operand stack at
    // [frame_base, frame_base + num_locals). The JIT stores locals in
    // Cranelift variables (CPU registers). When the JIT builds a
    // SuspendedFrame at yield, it must include local variable values
    // so the interpreter can find them on resume.
    //
    // outer has a local variable x=10. It calls inner, which yields.
    // After resume, x must still be 10.
    let source = r#"
        (def inner (fn () (yield 1) 2))
        (def outer (fn ()
          (let ((x 10))
            (let ((y (inner)))
              (+ x y)))))
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
    // v1 = 1 (yielded value from inner)
    // v2 = 10 + 2 = 12 (x survives yield, y = inner's return value)
    assert_eq!(format!("{}", result), "(1 12)");
}

#[test]
fn test_jit_yield_multiple_locals_survive() {
    // Multiple locals must all survive yield/resume.
    let source = r#"
        (def inner (fn () (yield 100) 200))
        (def outer (fn ()
          (let ((a 1))
            (let ((b 2))
              (let ((c 3))
                (let ((d (inner)))
                  (+ a (+ b (+ c d)))))))))
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
    // v1 = 100 (yielded)
    // v2 = 1 + 2 + 3 + 200 = 206
    assert_eq!(format!("{}", result), "(100 206)");
}
