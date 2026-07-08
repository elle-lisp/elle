use super::*;

// ============================================================================
// Test 1: Spawned closure with division by zero error
// ============================================================================

#[test]
fn test_spawned_closure_division_by_zero() {
    // Spawn a closure that will error (division by zero)
    // The error from the joined thread should be reported
    let result = eval_source(
        r#"
        (sys/join (sys/spawn-vm (fn () (/ 42 0))))
        "#,
        |r| r.map(|_| ()),
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
        (let [divisor 0]
          (sys/join (sys/spawn-vm (fn () (/ 42 divisor)))))
        "#,
        |r| r.map(|_| ()),
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
// Test 4: Multiple spawned closures - one errors
// ============================================================================

#[test]
fn test_multiple_spawned_closures_one_errors() {
    // Spawn multiple closures, one of which errors
    let result = eval_source(
        r#"
        (let [h1 (sys/spawn-vm (fn () 42))
              h2 (sys/spawn-vm (fn () (/ 1 0)))]
          (let [r1 (sys/join h1)]
            (sys/join h2)))
        "#,
        |r| r.map(|_| ()),
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
        (sys/join (sys/spawn-vm (fn () (+ "hello" 42))))
        "#,
        |r| r.map(|_| ()),
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
        (let [a 1 b 2 c 0]
          (sys/join (sys/spawn-vm (fn () (/ (+ a b) c)))))
        "#,
        |r| r.map(|_| ()),
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
    let source = "(fn (x) (numeric!) (%add x 1))";

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
        (sys/join (sys/spawn-vm (fn () (first 42))))
        "#,
        |r| r.map(|_| ()),
    );

    assert!(result.is_err(), "Expected type error from first");
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
// Test 13: Verify error propagation from spawned thread
// ============================================================================

#[test]
fn test_spawned_closure_error_propagation() {
    // Error should propagate from spawned thread to joining thread
    let result = eval_source(
        r#"
        (let [handle (sys/spawn-vm (fn () (/ 1 0)))]
          (sys/join handle))
        "#,
        |r| r.map(|_| ()),
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
    // `(numeric!)` sits inline on line 1 so the multi-line shape (and the
    // line numbers under test) is unchanged.
    let source = "(fn (x) (numeric!)\n  (%add x\n     1))";

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
// Property Tests: LocationMap preservation across thread transfer
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    /// Property: Closures compiled with location maps should have non-empty maps
    /// for any simple arithmetic expression.
    #[test]
    fn prop_closure_has_location_map(a in -100i64..100, b in -100i64..100) {
        let source = format!("(fn (x) (numeric!) (%add x {}))", a);
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
            "(let [captured {}] (sys/join (sys/spawn-vm (fn () (+ captured {})))))",
            a, b
        );
        let result = eval_source(&spawn_source, |r| r.map(|v| v.as_int()));
        prop_assert!(
            result.is_ok(),
            "Spawn/join should succeed for: {}",
            spawn_source
        );
        prop_assert_eq!(
            result.unwrap(),
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
        let source = format!("(sys/join (sys/spawn-vm (fn () (/ {} 0))))", a);
        let result = eval_source(&source, |r| r.map(|_| ()));

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
            "(let [x {} y {} z {}] (sys/join (sys/spawn-vm (fn () (+ x (+ y z))))))",
            a, b, c
        );
        let result = eval_source(&source, |r| r.map(|v| v.as_int()));

        prop_assert!(
            result.is_ok(),
            "Computation should succeed for: {}",
            source
        );
        prop_assert_eq!(
            result.unwrap(),
            Some(a + b + c),
            "Result should be {} + {} + {} = {}",
            a,
            b,
            c,
            a + b + c
        );
    }
}
