// Integration tests for colorless coroutines (issue #236)
// Tests all aspects of the coroutine implementation including:
// - Basic yield/resume
// - Coroutine state transitions
// - Effect inference
// - yield* delegation
// - Iterator protocol
// - Nested coroutines
// - Closures with captured variables
// - Error handling

use crate::common::eval_source;
use elle::Value;

/// Helper to collect integers from a cons list
#[allow(dead_code)]
fn collect_list_ints(value: &Value) -> Vec<i64> {
    let mut result = Vec::new();
    let mut current = value;
    while let Some(cons) = current.as_cons() {
        if let Some(n) = cons.first.as_int() {
            result.push(n);
        }
        current = &cons.rest;
    }
    result
}

// ============================================================================
// 1. BASIC YIELD/RESUME TESTS
// ============================================================================

#[test]
fn test_simple_yield() {
    // (var co (make-coroutine (fn () (yield 42))))
    // (coro/resume co) => 42
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (yield 42))))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_multiple_yields() {
    // (var co (make-coroutine (fn () (yield 1) (yield 2) (yield 3) 4)))
    // First resume => 1
    // Second resume => 2
    // Third resume => 3
    // Fourth resume => 4 (final value)
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (yield 1) (yield 2) (yield 3) 4)))
        (list
          (coro/resume co)
          (coro/resume co)
          (coro/resume co)
          (coro/resume co))
        "#,
    );
    assert!(result.is_ok(), "Multiple yields should work");
    let list_vals = collect_list_ints(&result.unwrap());
    assert_eq!(
        list_vals,
        vec![1, 2, 3, 4],
        "Should yield 1, 2, 3, then return 4"
    );
}

#[test]
fn test_yield_with_resume_value() {
    // (var co (make-coroutine (fn () (+ 10 (yield 1)))))
    // (coro/resume co) => 1
    // (coro/resume co 5) => 15
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (+ 10 (yield 1)))))
        (list
          (coro/resume co)
          (coro/resume co 5))
        "#,
    );
    assert!(result.is_ok(), "Resume with value should work");
    let list_vals = collect_list_ints(&result.unwrap());
    assert_eq!(
        list_vals,
        vec![1, 15],
        "First resume yields 1, second resume with 5 returns 10+5=15"
    );
}

// ============================================================================
// 2. COROUTINE STATE TESTS
// ============================================================================

#[test]
fn test_coroutine_status_created() {
    // Check status is :created keyword initially
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () 42)))
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("created"));
}

#[test]
fn test_coroutine_status_done() {
    // After completion, status is :done keyword
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () 42)))
        (coro/resume co)
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("done"));
}

#[test]
fn test_coroutine_done_predicate() {
    // (coro/done? co) should return false initially, true after completion
    let result = eval_source(
        r#"
         (var co (make-coroutine (fn () 42)))
         (list
           (coro/done? co)
           (begin (coro/resume co) (coro/done? co)))
         "#,
    );
    assert!(result.is_ok());
    // Result is a list of two booleans: [false, true]
    if let Some(cons) = result.unwrap().as_cons() {
        assert_eq!(cons.first, Value::bool(false), "Initially not done");
        if let Some(cons2) = cons.rest.as_cons() {
            assert_eq!(cons2.first, Value::bool(true), "Done after resume");
        }
    }
}

#[test]
fn test_resume_done_coroutine_fails() {
    // Resuming a done coroutine should error
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () 42)))
        (coro/resume co)
        (coro/resume co)
        "#,
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("cannot resume completed coroutine"));
}

#[test]
fn test_coroutine_value_after_yield() {
    // (coro/value co) should return the last yielded value
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (yield 42))))
        (coro/resume co)
        (coro/value co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

// ============================================================================
// 3. EFFECT INFERENCE TESTS
// ============================================================================

#[test]
fn test_pure_function_no_cps() {
    // A function without yield should be pure
    // (def sum (fn (n) (if (<= n 0) 0 (+ n (sum (- n 1))))))
    // Should work normally, no CPS overhead
    let result = eval_source(
        r#"
         (def sum (fn (n)
           (if (<= n 0)
             0
             (+ n (sum (- n 1))))))
         (sum 5)
         "#,
    );
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_yielding_function_detected() {
    // A function with yield should have Effect::yields()
    // This is more of a compiler-level test, but we can verify it works
    let result = eval_source(
        r#"
         (def gen (fn ()
           (yield 1)
           (yield 2)))
         (var co (make-coroutine gen))
         (coro/resume co)
         "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_calling_yielding_function_propagates_effect() {
    // If f yields and g calls f, g should also yield
    let result = eval_source(
        r#"
         (def f (fn ()
           (yield 1)))
         (def g (fn ()
           (f)
           (yield 2)))
         (var co (make-coroutine g))
         (coro/resume co)
         "#,
    );
    // Should yield 1 from f, then 2 from g
    // Currently this yields 2 because f's yield doesn't propagate
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        Value::int(1),
        "Should yield 1 from inner function f"
    );
}

// ============================================================================
// 4. YIELD* DELEGATION TESTS
// ============================================================================

#[test]
fn test_yield_star() {
    // yield* delegates to sub-coroutine, forwarding all yielded values
    let result = eval_source(
        r#"
        (def sub (coro/new (fn ()
          (yield 1)
          (yield 2)
          (yield 3)
          :done)))
        (def main (coro/new (fn ()
          (yield 0)
          (yield* sub)
          (yield 99))))
        (var results (list))
        (while (not (coro/done? main))
          (begin
            (coro/resume main nil)
            (when (not (coro/done? main))
              (set! results (append results (list (coro/value main)))))))
        results
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    // Should yield: 0, 1, 2, 3, 99
    assert_eq!(format!("{:?}", result.unwrap()), "(0 1 2 3 99)");
}

#[test]
fn test_yield_star_simple() {
    // Basic yield* delegation — outer yields inner's values then completes
    let result = eval_source(
        r#"
        (def inner (coro/new (fn () (yield 10) (yield 20) :final)))
        (def outer (coro/new (fn () (yield* inner))))
        (coro/resume outer nil)
        (def v1 (coro/value outer))
        (coro/resume outer nil)
        (def v2 (coro/value outer))
        (coro/resume outer nil)
        (list v1 v2 (coro/done? outer) (coro/value outer))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    // outer yields 10, then 20, then completes with :final
    assert_eq!(format!("{:?}", result.unwrap()), "(10 20 true :final)");
}

// ============================================================================
// 5. ITERATOR PROTOCOL TESTS
// ============================================================================

#[test]
fn test_coroutine_as_iterator() {
    // (each x (make-coroutine (fn () (yield 1) (yield 2)))
    //   (display x))
    // Should iterate over yielded values
    let result = eval_source(
        r#"
        (var results (list))
        (each x (make-coroutine (fn () (yield 1) (yield 2)))
          (set! results (cons x results)))
        results
        "#,
    );
    // This test documents the expected behavior for iterator protocol
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_to_iterator() {
    // (coro/>iterator co) should convert a coroutine to an iterator
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (yield 1))))
        (var iter (coro/>iterator co))
        (coro? iter)
        "#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

// ============================================================================
// 6. NESTED COROUTINES TESTS
// ============================================================================

#[test]
fn test_nested_coroutines() {
    // Coroutine that creates and resumes another coroutine
    let result = eval_source(
        r#"
        (def inner-gen (fn () (yield 10)))
        (def outer-gen (fn ()
          (var inner-co (make-coroutine inner-gen))
          (yield (coro/resume inner-co))))
        (var co (make-coroutine outer-gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_nested_coroutines_multiple_levels() {
    // Three levels of nesting
    let result = eval_source(
        r#"
        (def level3 (fn () (yield 3)))
        (def level2 (fn ()
          (var co3 (make-coroutine level3))
          (yield (coro/resume co3))))
        (def level1 (fn ()
          (var co2 (make-coroutine level2))
          (yield (coro/resume co2))))
        (var co1 (make-coroutine level1))
        (coro/resume co1)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(3));
}

// ============================================================================
// 7. CLOSURES WITH CAPTURED VARIABLES TESTS
// ============================================================================

#[test]
fn test_coroutine_with_captured_variables() {
    // (let ((x 10))
    //   (var co (make-coroutine (fn () (yield x) (yield (+ x 1)))))
    //   ...)
    let result = eval_source(
        r#"
        (let ((x 10))
          (var co (make-coroutine (fn () (yield x))))
          (coro/resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_coroutine_with_multiple_captured_variables() {
    // Multiple captured variables
    let result = eval_source(
        r#"
        (let ((x 10) (y 20))
          (var co (make-coroutine (fn () (yield (+ x y)))))
          (coro/resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_coroutine_captures_mutable_state() {
    // Coroutine captures a mutable cell
    let result = eval_source(
        r#"
        (let ((counter (box 0)))
          (var co (make-coroutine (fn ()
            (box-set! counter (+ (unbox counter) 1))
            (yield (unbox counter)))))
          (coro/resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_closure_captured_var_after_resume_issue_258() {
    // Regression test for issue #258: Closure environment not restored after yield/resume
    // When a coroutine's closure captures variables from an outer scope,
    // those captured variables must remain accessible after yield and resume.
    //
    // The make-counter function returns a closure that captures 'start'.
    // The inner closure becomes the coroutine and must access 'start'
    // across multiple yield/resume cycles.
    let result = eval_source(
        r#"
        (def make-counter (fn (start)
          (fn ()
            (yield start)
            (yield (+ start 1))
            (yield (+ start 2)))))
        (var co-100 (make-coroutine (make-counter 100)))
        (list
          (coro/resume co-100)
          (coro/resume co-100)
          (coro/resume co-100))
        "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 14. ISSUE #259 REGRESSION TESTS - STATE MANAGEMENT
// ============================================================================

#[test]
fn test_interleaved_coroutines_issue_259() {
    // Regression test for issue #259: Coroutine reports "already running" incorrectly
    // Interleaved resume operations on different coroutines should work correctly.
    // Each coroutine should maintain independent state.
    let result = eval_source(
        r#"
        (def make-counter (fn (start)
          (fn ()
            (yield start)
            (yield (+ start 1))
            (yield (+ start 2)))))
        (var co-100 (make-coroutine (make-counter 100)))
        (var co-200 (make-coroutine (make-counter 200)))
        (list
          (coro/resume co-100)
          (coro/resume co-200)
          (coro/resume co-100)
          (coro/resume co-200)
          (coro/resume co-100)
          (coro/resume co-200))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_status_suspended_after_yield() {
    // Verify coroutine is in Suspended state (not Running) after yield
    let result = eval_source(
        r#"
        (def gen (fn () (yield 1) (yield 2)))
        (var co (make-coroutine gen))
        (coro/resume co)
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(
        result.unwrap(),
        Value::string("suspended"),
        "Coroutine should be suspended after yield"
    );
}

#[test]
fn test_coroutine_state_after_error_during_resume() {
    // If an error occurs during coroutine execution after a yield,
    // the state should transition to Error, not stay Running.
    let result = eval_source(
        r#"
        (def bad-gen (fn ()
          (yield 1)
          (/ 1 0)))
        (var co (make-coroutine bad-gen))
        (coro/resume co)
        (coro/resume co)
        "#,
    );
    // The second resume should error (division by zero)
    assert!(result.is_err(), "Division by zero should cause error");
}

#[test]
fn test_coroutine_state_error_not_running_after_failure() {
    // After a coroutine fails, its state should be :error, not :running
    let result = eval_source(
        r#"
        (def bad-gen (fn ()
          (yield 1)
          (undefined-variable-that-does-not-exist)))
        (var co (make-coroutine bad-gen))
        (coro/resume co)
        (let ((f (fiber/new (fn () (coro/resume co)) 1)))
          (fiber/resume f))
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("error"));
}

#[test]
fn test_multiple_coroutines_independent_state() {
    // Multiple coroutines should have completely independent state
    let result = eval_source(
        r#"
        (def gen1 (fn () (yield 'a) (yield 'b)))
        (def gen2 (fn () (yield 'x) (yield 'y)))
        (var co1 (make-coroutine gen1))
        (var co2 (make-coroutine gen2))
        (list
          (coro/status co1)
          (coro/status co2)
          (coro/resume co1)
          (coro/status co1)
          (coro/status co2)
          (coro/resume co2)
          (coro/status co1)
          (coro/status co2))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_nested_coroutine_resume_from_coroutine() {
    // A coroutine that resumes another coroutine should work correctly
    // and not cause state confusion between the two
    let result = eval_source(
        r#"
        (def inner-gen (fn () (yield 10) (yield 20)))
        (def outer-gen (fn ()
          (var inner-co (make-coroutine inner-gen))
          (yield (+ 1 (coro/resume inner-co)))
          (yield (+ 1 (coro/resume inner-co)))))
        (var outer-co (make-coroutine outer-gen))
        (list
          (coro/resume outer-co)
          (coro/resume outer-co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_state_not_stuck_running_on_cps_error() {
    // If error occurs before first yield, state should be :error, not stuck on :running
    let result = eval_source(
        r#"
        (def bad-start-gen (fn ()
          (+ undefined-at-start 1)
          (yield 1)))
        (var co (make-coroutine bad-start-gen))
        (let ((f (fiber/new (fn () (coro/resume co)) 1)))
          (fiber/resume f))
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("error"));
}

// ============================================================================
// 8. ERROR HANDLING TESTS
// ============================================================================

#[test]
fn test_error_in_coroutine() {
    // Coroutine that throws - should set state to Error
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (/ 1 0))))
        (coro/resume co)
        "#,
    );
    assert!(result.is_err());
}

#[test]
fn test_error_in_coroutine_status() {
    // After error, status should be :error keyword
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (/ 1 0))))
        (let ((f (fiber/new (fn () (coro/resume co)) 1)))
          (fiber/resume f))
        (keyword->string (coro/status co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("error"));
}

#[test]
fn test_cannot_resume_errored_coroutine() {
    // Cannot resume a coroutine that errored
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (/ 1 0))))
        (coro/resume co)
        "#,
    );
    assert!(result.is_err());
}

// ============================================================================
// 9. COROUTINE PREDICATES AND ACCESSORS
// ============================================================================

#[test]
fn test_coroutine_predicate() {
    // (coro? val) should return true for coroutines
    let result = eval_source(
        r#"
         (var co (make-coroutine (fn () 42)))
         (list
           (coro? co)
           (coro? 42)
           (coro? (fn () 42)))
         "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 10. INTEGRATION WITH OTHER LANGUAGE FEATURES
// ============================================================================

#[test]
fn test_coroutine_with_recursion() {
    // Coroutine that uses recursion
    let result = eval_source(
        r#"
        (def countdown (fn (n)
          (if (<= n 0)
            (yield 0)
            (begin
              (yield n)
              (countdown (- n 1))))))
        (var co (make-coroutine (fn () (countdown 3))))
        (coro/resume co)
        "#,
    );
    // Should yield 3
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_with_higher_order_functions() {
    // Coroutine that uses map, filter, etc.
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn ()
          (yield (map (fn (x) (* x 2)) (list 1 2 3))))))
         (coro/resume co)
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

// ============================================================================
// 11. EDGE CASES AND BOUNDARY CONDITIONS
// ============================================================================

#[test]
fn test_coroutine_with_no_yield() {
    // Coroutine that never yields
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () 42)))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_coroutine_with_nil_yield() {
    // Coroutine that yields nil
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (yield nil))))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_coroutine_with_complex_yielded_value() {
    // Coroutine that yields a complex value
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn ()
          (yield (list 1 2 3)))))
        (coro/resume co)
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_with_empty_body() {
    // Coroutine with empty body (just returns nil)
    let result = eval_source(
        r#"
         (var co (make-coroutine (fn () nil)))
         (coro/resume co)
         "#,
    );
    assert_eq!(result.unwrap(), Value::NIL);
}

// ============================================================================
// 12. CPS PATH TESTS
// ============================================================================

// Note: The CPS path is used when a closure has a yielding effect AND has
// source AST available. These tests verify the CPS infrastructure works
// correctly for coroutine execution.

#[test]
fn test_cps_simple_yield() {
    // This test exercises the CPS path since the closure yields
    let result = eval_source(
        r#"
        (def gen (fn () (yield 42)))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_if() {
    // Yield inside an if expression
    let result = eval_source(
        r#"
        (def gen (fn ()
            (if true
                (yield 1)
                (yield 2))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_cps_yield_in_else() {
    // Yield inside else branch
    let result = eval_source(
        r#"
        (def gen (fn ()
            (if false
                (yield 1)
                (yield 2))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_cps_yield_in_begin() {
    // Yield inside a begin expression
    let result = eval_source(
        r#"
        (def gen (fn ()
            (begin
                (yield 1)
                (yield 2))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_cps_yield_with_computation() {
    // Yield a computed value
    let result = eval_source(
        r#"
        (def gen (fn ()
            (yield (+ 10 20 12))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_let() {
    // Yield inside a let expression
    let result = eval_source(
        r#"
        (def gen (fn ()
            (let ((x 10))
                (yield x))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_cps_yield_with_captured_var() {
    // Yield with a captured variable
    let result = eval_source(
        r#"
        (let ((x 42))
            (def gen (fn () (yield x)))
            (var co (make-coroutine gen))
            (coro/resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_and() {
    // Yield inside an and expression
    let result = eval_source(
        r#"
        (def gen (fn ()
            (and true (yield 42))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_or() {
    // Yield inside an or expression (short-circuit)
    let result = eval_source(
        r#"
        (def gen (fn ()
            (or false (yield 42))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_cond() {
    // Yield inside a cond expression
    let result = eval_source(
        r#"
        (def gen (fn ()
            (cond
                (false (yield 1))
                (true (yield 2))
                (else (yield 3)))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

// ============================================================================
// 13. PERFORMANCE AND STRESS TESTS
// ============================================================================

#[test]
fn test_coroutine_with_large_yielded_value() {
    // Coroutine that yields a large value
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn ()
          (yield (list 1 2 3 4 5 6 7 8 9 10)))))
         (coro/resume co)
         "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_multiple_coroutines_independent() {
    // Multiple independent coroutines
    let result = eval_source(
        r#"
         (var co1 (make-coroutine (fn () (yield 1))))
         (var co2 (make-coroutine (fn () (yield 2))))
         (list
           (coro/resume co1)
           (coro/resume co2))
         "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 15. ISSUE #260 REGRESSION TESTS - QUOTED SYMBOLS IN YIELD
// ============================================================================

#[test]
fn test_yield_quoted_symbol_issue_260() {
    // Regression test for issue #260: Quoted symbols in yield treated as variable references
    // When a coroutine yields a quoted symbol like 'a, it should yield the symbol
    // as a value, not attempt to look it up as a variable.
    let result = eval_source(
        r#"
        (def gen-sym (fn () (yield 'a) (yield 'b) (yield 'c)))
        (var co (make-coroutine gen-sym))
        (list
          (coro/resume co)
          (coro/resume co)
          (coro/resume co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_yield_quoted_symbol_is_value_not_variable() {
    // Verify that yielded symbols are actual symbol values that can be
    // tested with symbol? predicate, not variable lookups
    let result = eval_source(
        r#"
        (def gen (fn () (yield 'test-symbol)))
        (var co (make-coroutine gen))
        (var result (coro/resume co))
        (symbol? result)
        "#,
    );
    assert_eq!(
        result.unwrap(),
        Value::bool(true),
        "Yielded quoted symbol should be a symbol value"
    );
}

#[test]
fn test_yield_various_literal_types() {
    // Test that various literal types can be yielded without being
    // misinterpreted as variable references
    let result = eval_source(
        r#"
        (def gen (fn ()
          (yield 'symbol-val)
          (yield 42)
          (yield "string")
          (yield true)
          (yield nil)))
        (var co (make-coroutine gen))
        (list
          (symbol? (coro/resume co))
          (number? (coro/resume co))
          (string? (coro/resume co))
          (coro/resume co)
          (coro/resume co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_yield_quoted_list() {
    // Quoted lists should also be yielded as values, not evaluated
    let result = eval_source(
        r#"
        (def gen (fn () (yield '(1 2 3))))
        (var co (make-coroutine gen))
        (coro/resume co)
        "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 16.5. YIELD WITH INTERMEDIATE VALUES ON STACK (Phase 4 - LIR Yield Terminator)
// ============================================================================

#[test]
fn test_yield_with_intermediate_values_on_stack() {
    // Test that intermediate values on the operand stack survive yield/resume.
    // In (+ 1 (yield 2) 3), the value 1 is pushed before yield, and must be
    // available after resume to complete the addition.
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (+ 1 (yield 2) 3))))
        (list
          (coro/resume co)
          (coro/resume co 10))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // First resume: yields 2
    // Second resume with 10: (+ 1 10 3) = 14
    assert_eq!(list_vals[0], 2, "First yield should be 2");
    assert_eq!(list_vals[1], 14, "Final result should be 1 + 10 + 3 = 14");
}

#[test]
fn test_yield_with_multiple_intermediate_values() {
    // Multiple intermediate values on the stack before yield
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (+ 1 2 (yield 3) 4 5))))
        (list
          (coro/resume co)
          (coro/resume co 100))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // First resume: yields 3
    // Second resume with 100: (+ 1 2 100 4 5) = 112
    assert_eq!(list_vals[0], 3, "First yield should be 3");
    assert_eq!(
        list_vals[1], 112,
        "Final result should be 1 + 2 + 100 + 4 + 5 = 112"
    );
}

#[test]
fn test_yield_in_nested_call_with_intermediate_values() {
    // Yield inside a nested call with intermediate values at multiple levels
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (* 2 (+ 1 (yield 5) 3)))))
        (list
          (coro/resume co)
          (coro/resume co 10))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // First resume: yields 5
    // Second resume with 10: (* 2 (+ 1 10 3)) = (* 2 14) = 28
    assert_eq!(list_vals[0], 5, "First yield should be 5");
    assert_eq!(
        list_vals[1], 28,
        "Final result should be 2 * (1 + 10 + 3) = 28"
    );
}

#[test]
fn test_multiple_yields_with_intermediate_values() {
    // Multiple yields in sequence, each with intermediate values
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn ()
          (+ (+ 1 (yield 2) 3)
             (+ 4 (yield 5) 6)))))
        (list
          (coro/resume co)
          (coro/resume co 10)
          (coro/resume co 20))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // First resume: yields 2
    // Second resume with 10: (+ 1 10 3) = 14, then yields 5
    // Third resume with 20: (+ 4 20 6) = 30, then (+ 14 30) = 44
    assert_eq!(list_vals[0], 2, "First yield should be 2");
    assert_eq!(list_vals[1], 5, "Second yield should be 5");
    assert_eq!(
        list_vals[2], 44,
        "Final result should be (1+10+3) + (4+20+6) = 44"
    );
}

// ============================================================================
// 16.6. RUNTIME EFFECT CHECKS (Pure closure warnings)
// ============================================================================

#[test]
fn test_make_coroutine_pure_closure_still_works() {
    // make-coroutine with a pure closure should still work (just warns to stderr)
    // The closure has Pure effect because it never yields
    let result = eval_source(
        r#"
        (let ((co (make-coroutine (fn () 42))))
          (coro/resume co))
        "#,
    );
    assert!(
        result.is_ok(),
        "Pure closure in coroutine should still work"
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_make_coroutine_yielding_closure_works() {
    // make-coroutine with a yielding closure — no warning expected
    let result = eval_source(
        r#"
        (let ((co (make-coroutine (fn () (yield 42)))))
          (coro/resume co))
        "#,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_coroutine_resume_pure_closure_completes_immediately() {
    // A pure closure in a coroutine completes on first resume without yielding
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () (+ 1 2 3))))
        (list
          (coro/resume co)
          (keyword->string (coro/status co)))
        "#,
    );
    assert!(result.is_ok());
    // Should return 6 and status should be "done"
    if let Some(cons) = result.unwrap().as_cons() {
        assert_eq!(cons.first, Value::int(6), "Pure closure should return 6");
        if let Some(cons2) = cons.rest.as_cons() {
            assert_eq!(
                cons2.first,
                Value::string("done"),
                "Status should be done after pure closure completes"
            );
        }
    }
}

// ============================================================================
// 17. DEEP CROSS-CALL YIELD TESTS (Phase 3 hardening)
// ============================================================================

#[test]
fn test_yield_across_three_call_levels() {
    // Yield propagates through 3 levels of function calls
    // When resuming, we pass a value that becomes the yield expression's result
    let result = eval_source(
        r#"
        (def a (fn (x) (yield (* x 2))))
        (def b (fn (x) (+ (a x) 1)))
        (def c (fn (x) (+ (b x) 1)))
        (var co (make-coroutine (fn () (c 10))))
        (list (coro/resume co) (coro/resume co 20))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // c(10) -> b(10) -> a(10) -> yield 20
    // resume with 20 -> a returns 20 -> b returns 21 -> c returns 22
    assert_eq!(list_vals[0], 20, "First yield should be 20");
    assert_eq!(list_vals[1], 22, "Final return should be 22");
}

#[test]
fn test_yield_in_tail_position() {
    // Yield in tail position of coroutine body
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn ()
          (yield 1)
          (yield 2))))
        (list
          (coro/resume co)
          (coro/resume co)
          (coro/resume co)
          (coro/status co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    // yields 1, 2, then the second yield's return (empty list), done
}

#[test]
fn test_deep_call_chain_with_multiple_yields() {
    // Multiple yields at different call depths
    let result = eval_source(
        r#"
        (def level1 (fn ()
          (yield 1)
          (level2)))
        (def level2 (fn ()
          (yield 2)
          (level3)))
        (def level3 (fn ()
          (yield 3)
          "done"))
        (var co (make-coroutine level1))
        (list
          (coro/resume co)
          (coro/resume co)
          (coro/resume co)
          (coro/resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}
