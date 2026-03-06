// Tests for coroutines
//
// Fixed example tests for yield across call boundaries, error cases,
// and state preservation. Property-based behavioral tests have been
// migrated to tests/elle/coroutines.lisp.

use crate::common::eval_reuse_bare as eval_source;
use elle::Value;

/// Helper to collect integers from a cons list
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
// Property 7: Yield across call boundaries (expected to fail - requires CPS rework)
// ============================================================================

#[test]
fn yield_across_call_boundaries() {
    // A helper function that yields a value, called from a coroutine
    let code = r#"
        (begin
            (def helper (fn (x) (yield (* x 2))))
            (def gen (fn () (helper 21)))
            (var co (make-coroutine gen))
            (coro/resume co))
    "#;

    let result = eval_source(code);
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn yield_across_two_call_levels() {
    // Yield propagates through two levels of function calls
    let code = r#"
        (begin
            (def inner (fn (x) (yield (* x 3))))
            (def outer (fn (x) (inner (+ x 1))))
            (def gen (fn () (outer 10)))
            (var co (make-coroutine gen))
            (coro/resume co))
    "#;

    let result = eval_source(code);
    // (outer 10) -> (inner 11) -> (yield 33)
    assert_eq!(result.unwrap(), Value::int(33));
}

#[test]
fn yield_across_call_then_resume_then_yield() {
    // Yield, resume, then yield again across call boundaries
    let code = r#"
        (begin
            (def helper (fn (x)
                (let ((first (yield x)))
                    (yield (+ first x)))))
            (def gen (fn () (helper 10)))
            (var co (make-coroutine gen))
            (list
                (coro/resume co)
                (coro/resume co 5)
                (coro/status co)))
    "#;

    let result = eval_source(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    let list_vals = collect_list_ints(&result.unwrap());
    // First yield: 10
    // Second yield: 5 + 10 = 15
    assert_eq!(list_vals[0], 10, "First yield should be 10");
    assert_eq!(list_vals[1], 15, "Second yield should be 15");
}

#[test]
fn yield_across_call_with_return_value() {
    // After yield, the helper returns a value that the caller uses
    let code = r#"
        (begin
            (def helper (fn (x)
                (yield x)
                (* x 2)))
            (def gen (fn ()
                (let ((result (helper 5)))
                    (+ result 100))))
            (var co (make-coroutine gen))
            (list
                (coro/resume co)
                (coro/resume co)
                (keyword->string (coro/status co))))
    "#;

    let result = eval_source(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    // First resume: yields 5
    // Second resume: helper returns 10, gen returns 110
    let mut current = &result.unwrap();
    let mut values = Vec::new();
    while let Some(cons) = current.as_cons() {
        values.push(cons.first);
        current = &cons.rest;
    }

    assert_eq!(values[0], Value::int(5), "First yield should be 5");
    assert_eq!(values[1], Value::int(110), "Final return should be 110");
    assert_eq!(values[2], Value::string("done"), "Status should be 'done'");
}

// ============================================================================
// Example Tests: Error Cases
// ============================================================================

#[test]
fn yield_outside_coroutine_errors() {
    // yield outside of a coroutine context should error
    let code = "(yield 42)";
    let result = eval_source(code);
    assert!(result.is_err(), "yield outside coroutine should error");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("yield") || err_msg.contains("coroutine"),
        "Error should mention yield or coroutine, got: {}",
        err_msg
    );
}

#[test]
fn resume_completed_coroutine_errors() {
    let code = r#"
        (begin
            (var co (make-coroutine (fn () 42)))
            (coro/resume co)
            (coro/resume co))
    "#;
    let result = eval_source(code);
    // Should error because coroutine is already done
    // The error is set via vm.current_exception, so we check for NIL return
    // or an error message
    if let Ok(val) = &result {
        // If it returns Ok, it should be NIL (error was set)
        assert_eq!(
            *val,
            Value::NIL,
            "Resuming completed coroutine should set exception"
        );
    }
    // If it returns Err, that's also acceptable
}

#[test]
fn coroutine_that_never_yields() {
    // A pure function wrapped as a coroutine should work
    let code = r#"
        (begin
            (var co (make-coroutine (fn () (+ 1 2 3))))
            (coro/resume co))
    "#;
    let result = eval_source(code);
    assert!(result.is_ok(), "Pure function as coroutine should work");
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn mutable_local_preserved_across_resume() {
    // A mutable local should preserve its value across yield/resume
    let code = r#"
        (begin
            (def gen (fn ()
                (let ((x 0))
                    (set x 10)
                    (yield x)
                    (set x (+ x 5))
                    (yield x)
                    x)))
            (var co (make-coroutine gen))
            (list
                (coro/resume co)
                (coro/resume co)
                (coro/resume co)))
    "#;
    let result = eval_source(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    let list_vals = collect_list_ints(&result.unwrap());
    assert_eq!(
        list_vals,
        vec![10, 15, 15],
        "Mutable local not preserved correctly"
    );
}

// ============================================================================
// Property: Effect threading verification
// ============================================================================

#[test]
fn effect_threading_yields_effect_on_closure() {
    // Verify that a closure containing yield has the Yields effect
    // We test this indirectly by checking that the coroutine works correctly
    let code = r#"
        (begin
            (def gen (fn () (yield 42) (yield 43) 44))
            (var co (make-coroutine gen))
            (keyword->string (coro/status co)))
    "#;
    let result = eval_source(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("created"),
        "Coroutine should be in 'created' state"
    );
}
