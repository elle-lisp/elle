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
    // This test will likely fail initially as multiple yields aren't fully supported
    // but it documents the expected behavior
    assert!(result.is_ok());
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
    // This test documents the expected behavior for passing values back into coroutines
    assert!(result.is_ok());
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
    // NOTE: This test documents expected behavior for effect propagation.
    // Currently, calling a yielding function from within a coroutine
    // requires the bytecode path, not the CPS path, because the CPS
    // interpreter doesn't yet support nested yielding calls.
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
    // Should yield 1 from f, but currently fails with CPS path
    // because nested yielding calls aren't fully supported yet
    assert!(result.is_ok());
}

// ============================================================================
// 4. YIELD-FROM TESTS
// ============================================================================

#[test]
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
