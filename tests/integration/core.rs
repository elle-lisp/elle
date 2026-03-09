// DEFENSE: Integration tests that require Rust-specific APIs
//
// Tests that can be expressed in pure Elle have been migrated to
// tests/elle/core.lisp. The tests below remain because they need:
// - Error message substring matching
// - halt primitive (terminates the VM, can't test in a script)
use crate::common::eval_source;
use elle::Value;

// ============================================================================
// Error message content tests — require Rust string inspection
// ============================================================================

#[test]
fn test_undefined_variable_error_shows_name() {
    // Issue #300: error message should show the variable name, not a SymbolId
    let result = eval_source("nonexistent-foo");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("nonexistent-foo"),
        "Error should contain variable name, got: {}",
        err
    );
    assert!(
        !err.contains("symbol #"),
        "Error should not contain raw SymbolId, got: {}",
        err
    );
}

// ============================================================================
// halt primitive — terminates the VM, cannot be tested in a script
// ============================================================================

#[test]
fn test_halt_returns_value() {
    let result = eval_source("(halt 42)");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_halt_returns_nil() {
    let result = eval_source("(halt)");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_halt_stops_execution() {
    let result = eval_source("(begin (halt 1) 2)");
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_halt_in_function() {
    let result = eval_source("(begin (def f (fn () (halt 99))) (f))");
    assert_eq!(result.unwrap(), Value::int(99));
}

#[test]
fn test_halt_with_complex_value() {
    let result = eval_source("(halt (list 1 2 3))");
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(1), Value::int(2), Value::int(3)]);
}
