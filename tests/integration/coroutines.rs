// Integration tests for colorless coroutines (issue #236)
// Tests all aspects of the coroutine implementation including:
// - Basic yield/resume
// - Coroutine state transitions
// - Effect inference
// - Yield-from delegation
// - Iterator protocol
// - Nested coroutines
// - Closures with captured variables
// - Error handling

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::{init_stdlib, register_primitives};
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
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

fn eval_with_stdlib(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);
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
    // (define co (make-coroutine (fn () (yield 42))))
    // (coroutine-resume co) => 42
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (yield 42))))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_multiple_yields() {
    // (define co (make-coroutine (fn () (yield 1) (yield 2) (yield 3) 4)))
    // First resume => 1
    // Second resume => 2
    // Third resume => 3
    // Fourth resume => 4 (final value)
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (yield 1) (yield 2) (yield 3) 4)))
        (list
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co))
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
    // (define co (make-coroutine (fn () (+ 10 (yield 1)))))
    // (coroutine-resume co) => 1
    // (coroutine-resume co 5) => 15
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (+ 10 (yield 1)))))
        (list
          (coroutine-resume co)
          (coroutine-resume co 5))
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
    // Check status is "created" initially
    let result = eval(
        r#"
        (define co (make-coroutine (fn () 42)))
        (coroutine-status co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("created"));
}

#[test]
fn test_coroutine_status_done() {
    // After completion, status is "done"
    let result = eval(
        r#"
        (define co (make-coroutine (fn () 42)))
        (coroutine-resume co)
        (coroutine-status co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("done"));
}

#[test]
fn test_coroutine_done_predicate() {
    // (coroutine-done? co) should return #f initially, #t after completion
    let result = eval(
        r#"
         (define co (make-coroutine (fn () 42)))
         (list
           (coroutine-done? co)
           (begin (coroutine-resume co) (coroutine-done? co)))
         "#,
    );
    assert!(result.is_ok());
    // Result is a list of two booleans: [#f, #t]
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn () 42)))
        (coroutine-resume co)
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("cannot resume completed coroutine"));
}

#[test]
fn test_coroutine_value_after_yield() {
    // (coroutine-value co) should return the last yielded value
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (yield 42))))
        (coroutine-resume co)
        (coroutine-value co)
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
    // (define sum (fn (n) (if (<= n 0) 0 (+ n (sum (- n 1))))))
    // Should work normally, no CPS overhead
    let result = eval(
        r#"
         (define sum (fn (n)
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
    // A function with yield should have Effect::Yields
    // This is more of a compiler-level test, but we can verify it works
    let result = eval(
        r#"
         (define gen (fn ()
           (yield 1)
           (yield 2)))
         (define co (make-coroutine gen))
         (coroutine-resume co)
         "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_calling_yielding_function_propagates_effect() {
    // If f yields and g calls f, g should also yield
    let result = eval(
        r#"
         (define f (fn ()
           (yield 1)))
         (define g (fn ()
           (f)
           (yield 2)))
         (define co (make-coroutine g))
         (coroutine-resume co)
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
// 4. YIELD-FROM TESTS
// ============================================================================

#[test]
#[ignore] // Requires CPS rework: yield-from delegation not fully implemented
fn test_yield_from_basic() {
    // (define inner (fn () (yield 1) (yield 2)))
    // (define outer (fn () (yield-from (make-coroutine inner)) (yield 3)))
    // Should yield 1, 2, 3
    let result = eval(
        r#"
        (define inner (fn () (yield 1) (yield 2)))
        (define outer (fn () (yield-from (make-coroutine inner)) (yield 3)))
        (define co (make-coroutine outer))
        (coroutine-resume co)
        "#,
    );
    // Should get the first yielded value from inner
    assert!(result.is_ok());
    // yield-from should delegate to inner coroutine, so first resume yields 1
    // Currently yields 3 because yield-from doesn't properly delegate
    assert_eq!(
        result.unwrap(),
        Value::int(1),
        "First yield-from should yield 1 from inner"
    );
}

#[test]
fn test_yield_from_completion() {
    // yield-from should return the final value of the sub-coroutine
    let result = eval(
        r#"
        (define inner (fn () (yield 1) 42))
        (define outer (fn () (yield-from (make-coroutine inner))))
        (define co (make-coroutine outer))
        (coroutine-resume co)
        "#,
    );
    // Should eventually return 42 (the final value of inner)
    assert!(result.is_ok());
}

// ============================================================================
// 5. ITERATOR PROTOCOL TESTS
// ============================================================================

#[test]
fn test_coroutine_as_iterator() {
    // (each x (make-coroutine (fn () (yield 1) (yield 2)))
    //   (display x))
    // Should iterate over yielded values
    let result = eval(
        r#"
        (define results (list))
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
    // (coroutine->iterator co) should convert a coroutine to an iterator
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (yield 1))))
        (define iter (coroutine->iterator co))
        (coroutine? iter)
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
    let result = eval(
        r#"
        (define inner-gen (fn () (yield 10)))
        (define outer-gen (fn ()
          (define inner-co (make-coroutine inner-gen))
          (yield (coroutine-resume inner-co))))
        (define co (make-coroutine outer-gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_nested_coroutines_multiple_levels() {
    // Three levels of nesting
    let result = eval(
        r#"
        (define level3 (fn () (yield 3)))
        (define level2 (fn ()
          (define co3 (make-coroutine level3))
          (yield (coroutine-resume co3))))
        (define level1 (fn ()
          (define co2 (make-coroutine level2))
          (yield (coroutine-resume co2))))
        (define co1 (make-coroutine level1))
        (coroutine-resume co1)
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
    //   (define co (make-coroutine (fn () (yield x) (yield (+ x 1)))))
    //   ...)
    let result = eval(
        r#"
        (let ((x 10))
          (define co (make-coroutine (fn () (yield x))))
          (coroutine-resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_coroutine_with_multiple_captured_variables() {
    // Multiple captured variables
    let result = eval(
        r#"
        (let ((x 10) (y 20))
          (define co (make-coroutine (fn () (yield (+ x y)))))
          (coroutine-resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_coroutine_captures_mutable_state() {
    // Coroutine captures a mutable cell
    let result = eval(
        r#"
        (let ((counter (box 0)))
          (define co (make-coroutine (fn ()
            (box-set! counter (+ (unbox counter) 1))
            (yield (unbox counter)))))
          (coroutine-resume co))
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
    let result = eval(
        r#"
        (define make-counter (fn (start)
          (fn ()
            (yield start)
            (yield (+ start 1))
            (yield (+ start 2)))))
        (define co-100 (make-coroutine (make-counter 100)))
        (list
          (coroutine-resume co-100)
          (coroutine-resume co-100)
          (coroutine-resume co-100))
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
    let result = eval(
        r#"
        (define make-counter (fn (start)
          (fn ()
            (yield start)
            (yield (+ start 1))
            (yield (+ start 2)))))
        (define co-100 (make-coroutine (make-counter 100)))
        (define co-200 (make-coroutine (make-counter 200)))
        (list
          (coroutine-resume co-100)
          (coroutine-resume co-200)
          (coroutine-resume co-100)
          (coroutine-resume co-200)
          (coroutine-resume co-100)
          (coroutine-resume co-200))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_status_suspended_after_yield() {
    // Verify coroutine is in Suspended state (not Running) after yield
    let result = eval(
        r#"
        (define gen (fn () (yield 1) (yield 2)))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        (coroutine-status co)
        "#,
    );
    assert_eq!(
        result.unwrap(),
        Value::string("suspended"),
        "Coroutine should be suspended after yield, not running"
    );
}

#[test]
fn test_coroutine_state_after_error_during_resume() {
    // If an error occurs during coroutine execution after a yield,
    // the state should transition to Error, not stay Running.
    let result = eval(
        r#"
        (define bad-gen (fn ()
          (yield 1)
          (/ 1 0)))
        (define co (make-coroutine bad-gen))
        (coroutine-resume co)
        (coroutine-resume co)
        "#,
    );
    // The second resume should error (division by zero)
    assert!(result.is_err(), "Division by zero should cause error");
}

#[test]
fn test_coroutine_state_error_not_running_after_failure() {
    // After a coroutine fails, its state should be "error", not "running"
    let result = eval(
        r#"
        (define bad-gen (fn ()
          (yield 1)
          (undefined-variable-that-does-not-exist)))
        (define co (make-coroutine bad-gen))
        (coroutine-resume co)
        (handler-case
          (coroutine-resume co)
          (error e nil))
        (coroutine-status co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("error"));
}

#[test]
fn test_multiple_coroutines_independent_state() {
    // Multiple coroutines should have completely independent state
    let result = eval(
        r#"
        (define gen1 (fn () (yield 'a) (yield 'b)))
        (define gen2 (fn () (yield 'x) (yield 'y)))
        (define co1 (make-coroutine gen1))
        (define co2 (make-coroutine gen2))
        (list
          (coroutine-status co1)
          (coroutine-status co2)
          (coroutine-resume co1)
          (coroutine-status co1)
          (coroutine-status co2)
          (coroutine-resume co2)
          (coroutine-status co1)
          (coroutine-status co2))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_nested_coroutine_resume_from_coroutine() {
    // A coroutine that resumes another coroutine should work correctly
    // and not cause state confusion between the two
    let result = eval(
        r#"
        (define inner-gen (fn () (yield 10) (yield 20)))
        (define outer-gen (fn ()
          (define inner-co (make-coroutine inner-gen))
          (yield (+ 1 (coroutine-resume inner-co)))
          (yield (+ 1 (coroutine-resume inner-co)))))
        (define outer-co (make-coroutine outer-gen))
        (list
          (coroutine-resume outer-co)
          (coroutine-resume outer-co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_state_not_stuck_running_on_cps_error() {
    // If error occurs before first yield, state should be "error", not stuck on "running"
    let result = eval(
        r#"
        (define bad-start-gen (fn ()
          (+ undefined-at-start 1)
          (yield 1)))
        (define co (make-coroutine bad-start-gen))
        (handler-case
          (coroutine-resume co)
          (error e nil))
        (coroutine-status co)
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (/ 1 0))))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_err());
}

#[test]
fn test_error_in_coroutine_status() {
    // After error, status should be "error"
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (/ 1 0))))
        (handler-case
          (coroutine-resume co)
          (error e nil))
        (coroutine-status co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::string("error"));
}

#[test]
fn test_cannot_resume_errored_coroutine() {
    // Cannot resume a coroutine that errored
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (/ 1 0))))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_err());
}

// ============================================================================
// 9. COROUTINE PREDICATES AND ACCESSORS
// ============================================================================

#[test]
fn test_coroutine_predicate() {
    // (coroutine? val) should return #t for coroutines
    let result = eval(
        r#"
         (define co (make-coroutine (fn () 42)))
         (list
           (coroutine? co)
           (coroutine? 42)
           (coroutine? (fn () 42)))
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
    let result = eval(
        r#"
        (define countdown (fn (n)
          (if (<= n 0)
            (yield 0)
            (begin
              (yield n)
              (countdown (- n 1))))))
        (define co (make-coroutine (fn () (countdown 3))))
        (coroutine-resume co)
        "#,
    );
    // Should yield 3
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_with_higher_order_functions() {
    // Coroutine that uses map, filter, etc.
    let result = eval_with_stdlib(
        r#"
        (define co (make-coroutine (fn ()
          (yield (map (fn (x) (* x 2)) (list 1 2 3))))))
         (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

#[test]
fn test_coroutine_with_exception_handling() {
    // Coroutine with try-catch
    let result = eval(
        r#"
         (define co (make-coroutine (fn ()
           (handler-case
             (yield (/ 1 0))
             (division-by-zero e (yield "error"))))))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 11. EDGE CASES AND BOUNDARY CONDITIONS
// ============================================================================

#[test]
fn test_coroutine_with_no_yield() {
    // Coroutine that never yields
    let result = eval(
        r#"
        (define co (make-coroutine (fn () 42)))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_coroutine_with_nil_yield() {
    // Coroutine that yields nil
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (yield nil))))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_coroutine_with_complex_yielded_value() {
    // Coroutine that yields a complex value
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (yield (list 1 2 3)))))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_coroutine_with_empty_body() {
    // Coroutine with empty body (just returns nil)
    let result = eval(
        r#"
         (define co (make-coroutine (fn () nil)))
         (coroutine-resume co)
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
    let result = eval(
        r#"
        (define gen (fn () (yield 42)))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_if() {
    // Yield inside an if expression
    let result = eval(
        r#"
        (define gen (fn ()
            (if #t
                (yield 1)
                (yield 2))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_cps_yield_in_else() {
    // Yield inside else branch
    let result = eval(
        r#"
        (define gen (fn ()
            (if #f
                (yield 1)
                (yield 2))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_cps_yield_in_begin() {
    // Yield inside a begin expression
    let result = eval(
        r#"
        (define gen (fn ()
            (begin
                (yield 1)
                (yield 2))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_cps_yield_with_computation() {
    // Yield a computed value
    let result = eval(
        r#"
        (define gen (fn ()
            (yield (+ 10 20 12))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_let() {
    // Yield inside a let expression
    let result = eval(
        r#"
        (define gen (fn ()
            (let ((x 10))
                (yield x))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_cps_yield_with_captured_var() {
    // Yield with a captured variable
    let result = eval(
        r#"
        (let ((x 42))
            (define gen (fn () (yield x)))
            (define co (make-coroutine gen))
            (coroutine-resume co))
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_and() {
    // Yield inside an and expression
    let result = eval(
        r#"
        (define gen (fn ()
            (and #t (yield 42))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_or() {
    // Yield inside an or expression (short-circuit)
    let result = eval(
        r#"
        (define gen (fn ()
            (or #f (yield 42))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_cps_yield_in_cond() {
    // Yield inside a cond expression
    let result = eval(
        r#"
        (define gen (fn ()
            (cond
                (#f (yield 1))
                (#t (yield 2))
                (else (yield 3)))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (yield (list 1 2 3 4 5 6 7 8 9 10)))))
         (coroutine-resume co)
         "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_multiple_coroutines_independent() {
    // Multiple independent coroutines
    let result = eval(
        r#"
         (define co1 (make-coroutine (fn () (yield 1))))
         (define co2 (make-coroutine (fn () (yield 2))))
         (list
           (coroutine-resume co1)
           (coroutine-resume co2))
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
    let result = eval(
        r#"
        (define gen-sym (fn () (yield 'a) (yield 'b) (yield 'c)))
        (define co (make-coroutine gen-sym))
        (list
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_yield_quoted_symbol_is_value_not_variable() {
    // Verify that yielded symbols are actual symbol values that can be
    // tested with symbol? predicate, not variable lookups
    let result = eval(
        r#"
        (define gen (fn () (yield 'test-symbol)))
        (define co (make-coroutine gen))
        (define result (coroutine-resume co))
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
    let result = eval(
        r#"
        (define gen (fn ()
          (yield 'symbol-val)
          (yield 42)
          (yield "string")
          (yield #t)
          (yield nil)))
        (define co (make-coroutine gen))
        (list
          (symbol? (coroutine-resume co))
          (number? (coroutine-resume co))
          (string? (coroutine-resume co))
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok());
}

#[test]
fn test_yield_quoted_list() {
    // Quoted lists should also be yielded as values, not evaluated
    let result = eval(
        r#"
        (define gen (fn () (yield '(1 2 3))))
        (define co (make-coroutine gen))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok());
}

// ============================================================================
// 16. HANDLER-CASE + YIELD TESTS (Phase 3 hardening)
// ============================================================================

#[test]
fn test_yield_inside_handler_case_then_exception() {
    // Bug fix test: handler-case should still catch exceptions after yield/resume
    // When yield occurs inside a handler-case body, the exception handler state
    // must be saved in the continuation and restored on resume.
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (handler-case
            (begin
              (yield 1)       ; handler is active here
              (/ 1 0))        ; after resume, handler should still be active
            (division-by-zero e "caught")))))
        (list
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let list_vals = collect_list_ints(&result.unwrap());
    // First resume: yields 1
    // Second resume: (/ 1 0) -> caught by handler-case -> "caught"
    // But "caught" is a string, not an int, so we check differently
    assert_eq!(list_vals[0], 1, "First yield should be 1");
    // The second element is "caught" string, not an int
}

#[test]
fn test_yield_inside_handler_case_string_result() {
    // Same as above but verify the string result
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (handler-case
            (begin
              (yield 1)
              (/ 1 0))
            (division-by-zero e "caught")))))
        (coroutine-resume co)
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("caught"),
        "Exception should be caught after resume"
    );
}

#[test]
fn test_yield_inside_nested_handler_case() {
    // Nested handler-case blocks should both survive yield/resume
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (handler-case
            (handler-case
              (begin
                (yield 1)
                (/ 1 0))
              (type-error e "inner-type-error"))
            (division-by-zero e "outer-div-zero")))))
        (list
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    // First resume yields 1, second resume triggers div-by-zero caught by outer handler
}

#[test]
fn test_handler_case_outside_coroutine_wrapping_yield() {
    // Handler-case outside the coroutine should not affect yield/resume
    // The handler is in the caller's context, not the coroutine's
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (yield 1)
          (/ 1 0))))
        (handler-case
          (begin
            (coroutine-resume co)
            (coroutine-resume co))
          (division-by-zero e "outer-caught"))
        "#,
    );
    // The outer handler-case should catch the exception from the coroutine
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("outer-caught"),
        "Outer handler should catch coroutine exception"
    );
}

#[test]
fn test_exception_before_yield_caught_normally() {
    // Exception before yield should be caught normally (no resume involved)
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (handler-case
            (begin
              (/ 1 0)
              (yield 1))
            (division-by-zero e "caught-before-yield")))))
        (coroutine-resume co)
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("caught-before-yield"),
        "Exception before yield should be caught"
    );
}

#[test]
fn test_multiple_yields_inside_handler_case() {
    // Multiple yields inside handler-case, exception after last yield
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (handler-case
            (begin
              (yield 1)
              (yield 2)
              (yield 3)
              (/ 1 0))
            (division-by-zero e "caught-after-3-yields")))))
        (list
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

#[test]
fn test_yield_across_call_with_handler_case() {
    // Yield from a called function, with handler-case in the caller
    let result = eval(
        r#"
        (define inner (fn () (yield 1) (/ 1 0)))
        (define co (make-coroutine (fn ()
          (handler-case
            (inner)
            (division-by-zero e "caught-from-inner")))))
        (list
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

// ============================================================================
// 16.5. YIELD WITH INTERMEDIATE VALUES ON STACK (Phase 4 - LIR Yield Terminator)
// ============================================================================

#[test]
fn test_yield_with_intermediate_values_on_stack() {
    // Test that intermediate values on the operand stack survive yield/resume.
    // In (+ 1 (yield 2) 3), the value 1 is pushed before yield, and must be
    // available after resume to complete the addition.
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (+ 1 (yield 2) 3))))
        (list
          (coroutine-resume co)
          (coroutine-resume co 10))
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (+ 1 2 (yield 3) 4 5))))
        (list
          (coroutine-resume co)
          (coroutine-resume co 100))
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn () (* 2 (+ 1 (yield 5) 3)))))
        (list
          (coroutine-resume co)
          (coroutine-resume co 10))
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (+ (+ 1 (yield 2) 3)
             (+ 4 (yield 5) 6)))))
        (list
          (coroutine-resume co)
          (coroutine-resume co 10)
          (coroutine-resume co 20))
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
// 17. DEEP CROSS-CALL YIELD TESTS (Phase 3 hardening)
// ============================================================================

#[test]
fn test_yield_across_three_call_levels() {
    // Yield propagates through 3 levels of function calls
    // When resuming, we pass a value that becomes the yield expression's result
    let result = eval(
        r#"
        (define a (fn (x) (yield (* x 2))))
        (define b (fn (x) (+ (a x) 1)))
        (define c (fn (x) (+ (b x) 1)))
        (define co (make-coroutine (fn () (c 10))))
        (list (coroutine-resume co) (coroutine-resume co 20))
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
    let result = eval(
        r#"
        (define co (make-coroutine (fn ()
          (yield 1)
          (yield 2))))
        (list
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-status co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    // yields 1, 2, then the second yield's return (empty list), done
}

#[test]
fn test_deep_call_chain_with_multiple_yields() {
    // Multiple yields at different call depths
    let result = eval(
        r#"
        (define level1 (fn ()
          (yield 1)
          (level2)))
        (define level2 (fn ()
          (yield 2)
          (level3)))
        (define level3 (fn ()
          (yield 3)
          "done"))
        (define co (make-coroutine level1))
        (list
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co)
          (coroutine-resume co))
        "#,
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}
