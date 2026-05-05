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
fn test_halt_returns_nil() {
    // (halt) with no args → NIL → Ok (clean exit)
    let result = eval_source("(halt)");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_halt_with_value_is_fatal() {
    // (halt <value>) → non-NIL → Err (fatal error, used for stack overflow etc.)
    let result = eval_source("(halt 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("42"));
}

#[test]
fn test_halt_no_args_stops_execution() {
    // (halt) stops execution and returns NIL
    let result = eval_source("(begin (halt) 2)");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_halt_with_value_stops_execution() {
    // (halt 1) stops execution with a fatal error (never reaches 2)
    let result = eval_source("(begin (halt 1) 2)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("1"));
}

#[test]
fn test_halt_with_value_in_function() {
    // (halt 99) inside a function → fatal error
    let result = eval_source("(begin (def f (fn () (halt 99))) (f))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("99"));
}
