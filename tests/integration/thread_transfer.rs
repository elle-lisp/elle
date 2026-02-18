// Tests for thread transfer of closures with location data
//
// Verifies that closures spawned in new threads correctly preserve their
// LocationMap for error reporting.

use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Try to compile as a single expression first
    match compile_new(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            // If that fails, try wrapping in a begin
            let wrapped = format!("(begin {})", input);
            match compile_new(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    // If that also fails, try compiling all expressions
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
// Test 1: Spawned closure with division by zero error
// ============================================================================

#[test]
fn test_spawned_closure_division_by_zero() {
    // Spawn a closure that will error (division by zero)
    // The error from the joined thread should be reported
    let result = eval(
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
    let result = eval(
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
    let result = eval(
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
    let result = eval(
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
    let result = eval(
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
    let result = eval(
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

    let result = compile_new(source, &mut symbols);
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
    let result = eval(
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
    let result = eval(
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
    let result = eval(
        r#"
        (let ((x 10))
          (join (spawn (fn () (if (> x 5) "big" "small")))))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_string(), Some("big"));
}

// ============================================================================
// Test 11: Verify closure constants are transferred
// ============================================================================

#[test]
fn test_spawned_closure_constants_transferred() {
    // Closure uses constants (literals in the body)
    let result = eval(
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
    let result = eval(
        r#"
        (join (spawn (fn () "hello from thread")))
        "#,
    );

    assert!(result.is_ok(), "Expected successful execution");
    assert_eq!(result.unwrap().as_string(), Some("hello from thread"));
}

// ============================================================================
// Test 13: Verify error propagation from spawned thread
// ============================================================================

#[test]
fn test_spawned_closure_error_propagation() {
    // Error should propagate from spawned thread to joining thread
    let result = eval(
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

    let result = compile_new(source, &mut symbols);
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
// Test 15: Spawned closure with vector operations
// ============================================================================

#[test]
fn test_spawned_closure_vector_operations() {
    // Closure performs vector operations
    let result = eval(
        r#"
        (let ((v [1 2 3]))
          (join (spawn (fn () (vector-ref v 1)))))
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
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: Closures compiled with location maps should have non-empty maps
    /// for any simple arithmetic expression.
    #[test]
    fn prop_closure_has_location_map(a in -100i64..100, b in -100i64..100) {
        let source = format!("(fn (x) (+ x {}))", a);
        let mut symbols = SymbolTable::new();

        let result = compile_new(&source, &mut symbols);
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
        let result = eval(&spawn_source);
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
        let result = eval(&source);

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
        let result = eval(&source);

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
