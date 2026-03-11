// Tests for thread transfer of closures with location data
//
// Verifies that closures spawned in new threads correctly preserve their
// LocationMap for error reporting.

use crate::common::eval_source;
use elle::pipeline::compile;
use elle::SymbolTable;

// ============================================================================
// Test 1: Spawned closure with division by zero error
// ============================================================================

#[test]
fn test_spawned_closure_division_by_zero() {
    // Spawn a closure that will error (division by zero)
    // The error from the joined thread should be reported
    let result = eval_source(
        r#"
        (join (spawn (fn () (/ 42 0))))
        "#,
    );

    // The result should be an error
    assert!(result.is_err(), "Expected error from division by zero");
    let error = result.unwrap_err();

    // The error should mention division by zero
    assert!(
        error.contains("division") || error.contains("zero") || error.contains("Division"),
        "Error should mention division by zero: {}",
        error
    );
}

// ============================================================================
// Test 2: Spawned closure with captures and division by zero
// ============================================================================

#[test]
fn test_spawned_closure_with_captures_division_by_zero() {
    // A closure that captures a variable, spawned to another thread, errors
    let result = eval_source(
        r#"
        (let ((divisor 0))
          (join (spawn (fn () (/ 42 divisor)))))
        "#,
    );

    // The result should be an error
    assert!(result.is_err(), "Expected error from division by zero");
    let error = result.unwrap_err();

    // The error should mention division by zero
    assert!(
        error.contains("division") || error.contains("zero") || error.contains("Division"),
        "Error should mention division by zero: {}",
        error
    );
}

// ============================================================================
// Test 3: Successful spawned closure still works
// ============================================================================

#[test]
fn test_spawned_closure_success() {
    // Even successful closures should work correctly
    let result = eval_source(
        r#"
        (let ((x 10) (y 20))
          (join (spawn (fn () (+ x y)))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_int(), Some(30));
}

// ============================================================================
// Test 4: Multiple spawned closures - one errors
// ============================================================================

#[test]
fn test_multiple_spawned_closures_one_errors() {
    // Spawn multiple closures, one of which errors
    let result = eval_source(
        r#"
        (let ((h1 (spawn (fn () 42)))
              (h2 (spawn (fn () (/ 1 0)))))
          (let ((r1 (join h1)))
            (join h2)))
        "#,
    );

    // The second join should error
    assert!(result.is_err(), "Expected error from division by zero");
}

// ============================================================================
// Test 5: Spawned closure with type error
// ============================================================================

#[test]
fn test_spawned_closure_type_error() {
    // A closure that causes a type error in the spawned thread
    let result = eval_source(
        r#"
        (join (spawn (fn () (+ "hello" 42))))
        "#,
    );

    // The result should be an error
    assert!(result.is_err(), "Expected type error");
    let error = result.unwrap_err();

    // The error should mention type mismatch
    assert!(
        error.contains("type")
            || error.contains("Type")
            || error.contains("expected")
            || error.contains("string"),
        "Error should mention type issue: {}",
        error
    );
}

// ============================================================================
// Test 6: Closure with multiple captures and error
// ============================================================================

#[test]
fn test_closure_with_multiple_captures_and_error() {
    // A closure that captures multiple values and then errors
    let result = eval_source(
        r#"
        (let ((a 1) (b 2) (c 0))
          (join (spawn (fn () (/ (+ a b) c)))))
        "#,
    );

    // The result should be an error
    assert!(result.is_err(), "Expected error from division by zero");
}

// ============================================================================
// Test 7: Verify location map is non-empty for compiled closure
// ============================================================================

#[test]
fn test_compiled_closure_has_location_map() {
    let mut symbols = SymbolTable::new();
    let source = "(fn (x) (+ x 1))";

    let result = compile(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");

    let compiled = result.unwrap();
    // The main bytecode should have a location map
    assert!(
        !compiled.bytecode.location_map.is_empty(),
        "Compiled bytecode should have non-empty LocationMap"
    );
}

// ============================================================================
// Test 8: Spawned closure error message is informative
// ============================================================================

#[test]
fn test_spawned_closure_error_message_format() {
    // Verify that errors from spawned threads have reasonable formatting
    let result = eval_source(
        r#"
        (join (spawn (fn () (car 42))))
        "#,
    );

    assert!(result.is_err(), "Expected type error from car");
    let error = result.unwrap_err();

    // Error should be informative
    assert!(!error.is_empty(), "Error message should not be empty");
    assert!(
        error.len() > 5,
        "Error message should be descriptive: {}",
        error
    );
}

// ============================================================================
// Test 9: Spawned closure with captured computation
// ============================================================================

#[test]
fn test_spawned_closure_captured_computation() {
    // Closure captures computed values
    let result = eval_source(
        r#"
        (let ((x (+ 1 2))
              (y (* 3 4)))
          (join (spawn (fn () (+ x y)))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_int(), Some(15)); // 3 + 12 = 15
}

// ============================================================================
// Test 10: Spawned closure with conditional
// ============================================================================

#[test]
fn test_spawned_closure_with_conditional() {
    // Closure uses conditional logic
    let result = eval_source(
        r#"
        (let ((x 10))
          (join (spawn (fn () (if (> x 5) "big" "small")))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().with_string(|s| s == "big"), Some(true));
}

// ============================================================================
// Test 11: Verify closure constants are transferred
// ============================================================================

#[test]
fn test_spawned_closure_constants_transferred() {
    // Closure uses constants (literals in the body)
    let result = eval_source(
        r#"
        (join (spawn (fn () (+ 100 200))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_int(), Some(300));
}

// ============================================================================
// Test 12: Verify closure with string constant
// ============================================================================

#[test]
fn test_spawned_closure_string_constant() {
    // Closure returns a string constant
    let result = eval_source(
        r#"
        (join (spawn (fn () "hello from thread")))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(
        result.unwrap().with_string(|s| s == "hello from thread"),
        Some(true)
    );
}

// ============================================================================
// Test 13: Verify error propagation from spawned thread
// ============================================================================

#[test]
fn test_spawned_closure_error_propagation() {
    // Error should propagate from spawned thread to joining thread
    let result = eval_source(
        r#"
        (let ((handle (spawn (fn () (/ 1 0)))))
          (join handle))
        "#,
    );

    assert!(
        result.is_err(),
        "Error should propagate from spawned thread"
    );
}

// ============================================================================
// Test 14: Verify location map entries have valid line numbers
// ============================================================================

#[test]
fn test_location_map_has_valid_line_numbers() {
    let mut symbols = SymbolTable::new();
    // Multi-line source to verify line tracking
    let source = "(fn (x)\n  (+ x\n     1))";

    let result = compile(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");

    let compiled = result.unwrap();

    // All entries should have line >= 1
    for loc in compiled.bytecode.location_map.values() {
        assert!(
            loc.line >= 1,
            "Line numbers should be >= 1, got {}",
            loc.line
        );
    }
}

// ============================================================================
// Test 15: Spawned closure with array operations
// ============================================================================

#[test]
fn test_spawned_closure_array_operations() {
    // Closure performs array operations
    let result = eval_source(
        r#"
        (let ((v @[1 2 3]))
          (join (spawn (fn () (get v 1)))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_int(), Some(2));
}

// ============================================================================
// Property Tests: LocationMap preservation across thread transfer
// ============================================================================

use proptest::prelude::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    /// Property: Closures compiled with location maps should have non-empty maps
    /// for any simple arithmetic expression.
    #[test]
    fn prop_closure_has_location_map(a in -100i64..100, b in -100i64..100) {
        let source = format!("(fn (x) (+ x {}))", a);
        let mut symbols = SymbolTable::new();

        let result = compile(&source, &mut symbols, "<test>");
        prop_assert!(result.is_ok(), "Compilation should succeed for: {}", source);

        let compiled = result.unwrap();
        prop_assert!(
            !compiled.bytecode.location_map.is_empty(),
            "LocationMap should be non-empty for closure: {}",
            source
        );

        // Also verify all line numbers are valid
        for loc in compiled.bytecode.location_map.values() {
            prop_assert!(
                loc.line >= 1,
                "Line number should be >= 1, got {} for source: {}",
                loc.line,
                source
            );
        }

        // Now test that spawning and joining preserves the computation
        let spawn_source = format!(
            "(let ((captured {})) (join (spawn (fn () (+ captured {})))))",
            a, b
        );
        let result = eval_source(&spawn_source);
        prop_assert!(
            result.is_ok(),
            "Spawn/join should succeed for: {}",
            spawn_source
        );
        prop_assert_eq!(
            result.unwrap().as_int(),
            Some(a + b),
            "Result should be {} + {} = {}",
            a,
            b,
            a + b
        );
    }

    /// Property: Spawned closures should correctly propagate division by zero errors
    #[test]
    fn prop_spawned_closure_propagates_div_by_zero(a in 1i64..100) {
        let source = format!("(join (spawn (fn () (/ {} 0))))", a);
        let result = eval_source(&source);

        prop_assert!(
            result.is_err(),
            "Division by zero should error for: {}",
            source
        );

        let error = result.unwrap_err();
        prop_assert!(
            error.contains("division") || error.contains("zero") || error.contains("Division"),
            "Error should mention division by zero: {}",
            error
        );
    }

    /// Property: Spawned closures with captures should compute correctly
    #[test]
    fn prop_spawned_closure_with_captures_computes_correctly(
        a in -50i64..50,
        b in -50i64..50,
        c in -50i64..50
    ) {
        let source = format!(
            "(let ((x {}) (y {}) (z {})) (join (spawn (fn () (+ x (+ y z))))))",
            a, b, c
        );
        let result = eval_source(&source);

        prop_assert!(
            result.is_ok(),
            "Computation should succeed for: {}",
            source
        );
        prop_assert_eq!(
            result.unwrap().as_int(),
            Some(a + b + c),
            "Result should be {} + {} + {} = {}",
            a,
            b,
            c,
            a + b + c
        );
    }
}

// ============================================================================
// Test 16: Closure capturing another closure
// ============================================================================

#[test]
fn test_closure_capturing_closure() {
    let result = eval_source(
        r#"
        (let ((add1 (fn (x) (+ x 1))))
          (join (spawn (fn () (add1 41)))))
        "#,
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_int(), Some(42));
}

// ============================================================================
// Test 17: Closure capturing nested closures (three levels)
// ============================================================================

#[test]
fn test_closure_capturing_nested_closures() {
    let result = eval_source(
        r#"
        (let ((add1 (fn (x) (+ x 1))))
          (let ((add2 (fn (x) (add1 (add1 x)))))
            (join (spawn (fn () (add2 40))))))
        "#,
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_int(), Some(42));
}

// ============================================================================
// Test 18: Closure capturing non-sendable value via inner closure is rejected
// ============================================================================

#[test]
fn test_closure_capturing_non_sendable_rejected() {
    // A closure that captures a mutable @struct (via an inner closure) is rejected.
    let result = eval_source(
        r#"
        (let ((t (@struct)))
          (let ((f (fn () t)))
            (spawn (fn () (f)))))
        "#,
    );

    // spawn should error because f captures a mutable @struct.
    assert!(
        result.is_err(),
        "Expected spawn to fail for non-sendable transitive capture"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("@struct") || err.contains("struct") || err.contains("mutable"),
        "Error should mention @struct: {}",
        err
    );
}

// ============================================================================
// Test 19: Spawned closure returning a closure as its result
// ============================================================================

#[test]
fn test_closure_result_is_closure() {
    let result = eval_source(
        r#"
        (let ((f (join (spawn (fn () (fn (x) (+ x 1)))))))
          (f 41))
        "#,
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_int(), Some(42));
}

// ============================================================================
// Test 20: Self-recursive closure via letrec (factorial)
// ============================================================================

#[test]
fn test_self_recursive_closure() {
    let result = eval_source(
        r#"
        (letrec ((fact (fn (n) (if (= n 0) 1 (* n (fact (- n 1)))))))
          (join (spawn (fn () (fact 5)))))
        "#,
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_int(), Some(120));
}

// ============================================================================
// Test 21: Mutually recursive closures via letrec (even?/odd?)
// ============================================================================

#[test]
fn test_mutually_recursive_closures() {
    let result = eval_source(
        r#"
        (letrec ((even? (fn (n) (if (= n 0) true (odd? (- n 1)))))
                 (odd?  (fn (n) (if (= n 0) false (even? (- n 1))))))
          (join (spawn (fn () (even? 10)))))
        "#,
    );

    assert!(
        result.is_ok(),
        "Expected successful execution, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().as_bool(), Some(true));
}
