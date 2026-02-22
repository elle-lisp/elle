// Integration tests for lexical scope refactor
// Tests comprehensive capture behavior across nested scopes, let bindings,
// mutable captures, and coroutine interactions.

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);

    match compile_new(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            let wrapped = format!("(begin {})", input);
            match compile_new(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    let results = compile_all_new(input, &mut symbols)?;
                    let mut last_result = Value::NIL;
                    for result in results {
                        last_result = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
                    }
                    Ok(last_result)
                }
            }
        }
    }
}

// ============================================================================
// SECTION 1: Deeply Nested Captures (4+ levels)
// ============================================================================

#[test]
fn test_capture_from_great_grandparent() {
    // 4 levels: a -> b -> c -> d, innermost captures from outermost
    let code = r#"
        (((((fn (a) (fn (b) (fn (c) (fn (d) (+ a b c d))))) 1) 2) 3) 4)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

#[test]
fn test_capture_skip_levels() {
    // Inner captures from grandparent, skipping parent
    let code = r#"
        ((((fn (x) (fn (y) (fn (z) (+ x z)))) 10) 20) 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_five_level_nesting() {
    // 5 levels of nesting with captures at each level
    let code = r#"
        ((((((fn (a) (fn (b) (fn (c) (fn (d) (fn (e) (+ a b c d e)))))) 1) 2) 3) 4) 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_capture_alternating_levels() {
    // Capture from alternating levels (skip one, capture one)
    let code = r#"
        (((((fn (a) (fn (b) (fn (c) (fn (d) (+ a c))))) 10) 20) 30) 40)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(40));
}

#[test]
fn test_deeply_nested_all_params() {
    // All parameters used in innermost function
    let code = r#"
        (((((fn (a) (fn (b) (fn (c) (fn (d) (* a (+ b (- c d))))))) 2) 3) 4) 1)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(12)); // 2 * (3 + (4 - 1)) = 2 * 6 = 12
}

// ============================================================================
// SECTION 2: Mixed Let/Lambda Captures
// ============================================================================

#[test]
fn test_let_inside_lambda_capture() {
    let code = r#"
        (let ((f ((fn (x)
                    (let ((y 10))
                      (fn () (+ x y)))) 5)))
          (f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_nested_let_lambda_let() {
    let code = r#"
        (let ((a 1))
          (let ((f ((fn (b)
                      (let ((c 3))
                        (fn () (+ a b c)))) 2)))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_lambda_captures_let_binding() {
    let code = r#"
        (let ((x 5))
          (let ((f (fn () x)))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_multiple_lambdas_same_let_scope() {
    let code = r#"
        (let ((x 10) (y 20))
          (let ((f1 (fn () x))
                (f2 (fn () y)))
            (+ (f1) (f2))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_lambda_in_let_captures_outer_let() {
    let code = r#"
        (let ((outer 100))
          (let ((inner 50))
            (let ((f (fn () (+ outer inner))))
              (f))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(150));
}

#[test]
fn test_let_star_with_lambda_capture() {
    let code = r#"
        (let* ((x 1)
               (y (+ x 1))
               (f (fn () (+ x y))))
          (f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 3: Mutable Capture Edge Cases
// ============================================================================

#[test]
fn test_set_on_let_bound_capture() {
    let code = r#"
        (let ((x 0))
          (let ((inc (fn () (begin (set! x (+ x 1)) x))))
            (begin (inc) (inc) (inc))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_set_on_locally_defined_capture() {
    let code = r#"
        ((fn ()
           (begin
             (define counter 0)
             (define inc (fn () (begin (set! counter (+ counter 1)) counter)))
             (begin (inc) (inc) (inc)))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_multiple_closures_share_mutable_capture() {
    let code = r#"
        (let ((x 0))
          (let ((inc (fn () (set! x (+ x 1))))
                (get (fn () x)))
            (begin (inc) (inc) (get))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(2));
}

#[test]
fn test_nested_mutable_captures() {
    let code = r#"
        (let ((x 0))
          (let ((f (fn () (let ((y 0))
                            (fn () (begin (set! x (+ x 1)) (set! y (+ y 1)) (+ x y)))))))
            (let ((g (f)))
              (begin (g) (g) (g)))))
    "#;
    // x increments 3 times (shared), y increments 3 times (local to g)
    // Final: x=3, y=3, result=6
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_mutable_capture_across_lambda_levels() {
    let code = r#"
        (let ((counter 0))
          (let ((f (fn () (fn () (begin (set! counter (+ counter 1)) counter)))))
            (let ((g (f)))
              (begin (g) (g) (g)))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_multiple_mutable_captures() {
    let code = r#"
        (let ((x 0) (y 0))
          (let ((inc-x (fn () (set! x (+ x 1))))
                (inc-y (fn () (set! y (+ y 1))))
                (sum (fn () (+ x y))))
            (begin (inc-x) (inc-y) (inc-x) (sum))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 4: CPS/Coroutine Captures
// ============================================================================

#[test]
fn test_coroutine_captures_from_nested_let() {
    let code = r#"
        (let ((x 10))
          (let ((y 20))
            (let ((gen (fn () (yield (+ x y)))))
              (let ((co (make-coroutine gen)))
                (coro/resume co)))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_coroutine_captures_lambda_param() {
    let code = r#"
        ((fn (base)
           (let ((gen (fn () (yield base))))
             (let ((co (make-coroutine gen)))
               (coro/resume co)))) 42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_coroutine_captures_multiple_levels() {
    let code = r#"
        ((fn (a)
           ((fn (b)
              (let ((gen (fn () (yield (+ a b)))))
                (let ((co (make-coroutine gen)))
                  (coro/resume co)))) 20)) 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_coroutine_with_mutable_capture() {
    let code = r#"
        (let ((counter 0))
          (let ((gen (fn () (begin (set! counter (+ counter 1)) (yield counter)))))
            (let ((co (make-coroutine gen)))
              (coro/resume co))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(1));
}

#[test]
fn test_coroutine_captures_let_star_binding() {
    let code = r#"
        (let* ((x 5)
               (y (+ x 10))
               (gen (fn () (yield (+ x y))))
               (co (make-coroutine gen)))
          (coro/resume co))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

// ============================================================================
// SECTION 5: Complex Interaction Tests
// ============================================================================

#[test]
fn test_closure_returning_closure_with_captures() {
    let code = r#"
        (let ((x 5))
          (let ((f (fn () (fn () x))))
            (let ((g (f)))
              (g))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_shadowing_in_nested_scopes() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn (x) (fn () x))))
            (let ((g (f 20)))
              (g))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

#[test]
fn test_capture_with_shadowing_outer() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn () (let ((x 20)) (fn () x)))))
            (let ((g (f)))
              (g))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

#[test]
fn test_multiple_captures_same_variable() {
    let code = r#"
        (let ((x 5))
          (let ((f (fn () x))
                (g (fn () (+ x x))))
            (+ (f) (g))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_capture_in_conditional() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn (cond) (if cond (fn () x) (fn () 0)))))
            (let ((g (f #t)))
              (g))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

#[test]
fn test_capture_in_loop_body() {
    let code = r#"
        (let ((x 0))
          (let ((f (fn () (begin (set! x (+ x 1)) x))))
            (begin (f) (f) (f) x)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 6: Edge Cases and Stress Tests
// ============================================================================

#[test]
fn test_empty_lambda_capture() {
    // Lambda with no parameters that captures nothing
    let code = r#"
        ((fn () 42))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_lambda_unused_parameter() {
    // Parameter exists but isn't used
    let code = r#"
        ((fn (x) 42) 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_capture_unused_let_binding() {
    // Let binding exists but closure doesn't capture it
    let code = r#"
        (let ((x 10) (y 20))
          (let ((f (fn () x)))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

#[test]
fn test_many_captures_same_closure() {
    // Single closure capturing many variables
    let code = r#"
        (let ((a 1) (b 2) (c 3) (d 4) (e 5))
          (let ((f (fn () (+ a b c d e))))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_capture_in_nested_let_star() {
    let code = r#"
        (let* ((a 1)
               (b (+ a 1))
               (c (+ b 1))
               (f (fn () (+ a b c))))
          (f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_lambda_param_shadows_let_binding() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn (x) (+ x 5))))
            (f 20)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(25));
}

#[test]
fn test_nested_lambda_param_shadowing() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn (x) (fn (x) x))))
            (let ((g (f 20)))
              (g 30))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_capture_with_define_in_lambda() {
    let code = r#"
        (let ((x 10))
          (let ((f (fn () (begin (define y (+ x 5)) y))))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_mutual_recursion_with_captures() {
    let code = r#"
        (let ((limit 4))
          (begin
            (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
            (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
            (is-even limit)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_capture_across_define_boundary() {
    let code = r#"
        (let ((x 10))
          (begin
            (define f (fn () x))
            (f)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

// ============================================================================
// SECTION 7: Regression Tests for Locally-Defined Variables
// ============================================================================

#[test]
fn test_self_recursive_function_via_define_inside_fn() {
    // Bug 1: Self-recursive function defined inside fn body
    let code = r#"
        ((fn (n)
           (begin
             (define fact (fn (x) (if (= x 0) 1 (* x (fact (- x 1))))))
             (fact n))) 6)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(720));
}

#[test]
fn test_nested_lambda_capturing_locally_defined_variable() {
    // Bug 2: Nested lambda capturing locally-defined variable
    let code = r#"
        ((fn ()
           (begin
             (define x 42)
             (define f (fn () x))
             (f))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_multiple_closures_sharing_mutable_state_via_define() {
    // Bug 3: Multiple closures sharing mutable state via define
    let code = r#"
        ((fn (initial)
           (begin
             (define value initial)
             (define getter (fn () value))
             (define setter (fn (new-val) (set! value new-val)))
             (setter 42)
             (getter))) 0)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_mutual_recursion_via_define_inside_fn() {
    // Mutual recursion via define inside fn
    let code = r#"
        ((fn ()
           (begin
             (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
             (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
             (is-even 8))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

// ============================================================================
// REGRESSION: let/letrec inside closures must use StoreCapture, not StoreLocal
// ============================================================================
// These tests guard against a bug where `let` and `letrec` bindings inside
// lambdas used StoreLocal/LoadLocal (stack-based) instead of
// StoreCapture/LoadCapture (environment-based). The StoreLocal writes would
// corrupt the caller's stack, because frame_base=0 at top level means
// StoreLocal(0, idx) writes to stack[idx] - the same slots used by
// the caller's local variables.

#[test]
fn test_let_inside_closure_does_not_corrupt_caller_stack() {
    // The bug: a closure with an internal `let` would overwrite the
    // caller's locals when the `let` binding used StoreLocal instead
    // of StoreCapture. This test has a top-level `let` binding `x`,
    // then calls a closure that has its own internal `let` binding.
    // After the call, `x` must still be intact.
    let code = r#"
        (begin
          (define check (fn (val)
            (let ((temp (+ val 1)))
              temp)))
          (let ((x 100))
            (check 5)
            x))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(100));
}

#[test]
fn test_let_inside_closure_returns_correct_value() {
    // The closure's let binding must be stored in its own environment,
    // not on the shared stack.
    let code = r#"
        (begin
          (define f (fn (a b)
            (let ((sum (+ a b))
                  (diff (- a b)))
              (+ sum diff))))
          (f 10 3))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

#[test]
fn test_letrec_inside_closure_does_not_corrupt_caller_stack() {
    // Same bug as above but for letrec.
    let code = r#"
        (begin
          (define process (fn (n)
            (letrec ((helper (fn (x) (if (= x 0) 0 (+ x (helper (- x 1)))))))
              (helper n))))
          (let ((result 999))
            (process 5)
            result))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(999));
}

#[test]
fn test_multiple_closures_with_let_dont_interfere() {
    // Multiple closures with internal let bindings called in sequence.
    // Each must use its own environment, not stomp the stack.
    let code = r#"
        (begin
          (define f (fn (x) (let ((a (+ x 1))) a)))
          (define g (fn (x) (let ((b (* x 2))) b)))
          (let ((r1 (f 10))
                (r2 (g 20)))
            (+ r1 r2)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(51));
}

#[test]
fn test_closure_let_with_string_operations() {
    // This is the pattern that originally failed in concurrency.lisp:
    // string-contains? (which uses let internally in assert-eq) was
    // receiving a boolean instead of a string after spawn/join.
    let code = r#"
        (begin
          (define checker (fn (s)
            (let ((result (string-contains? s "hello")))
              result)))
          (let ((msg "say hello world"))
            (checker msg)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}
