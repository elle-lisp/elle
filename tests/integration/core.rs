// audited: 2026-09-29
// The core behaviors a script cannot check: an undefined-variable error's text, and `halt`.
//
// docs/errors.md
// docs/signals/fibers.md
//
// tests/lang/core.lisp holds the rest. These need the error string, or they end
// the VM that a script would read its result from.

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// Error message content tests — require Rust string inspection
// ============================================================================

#[test]
fn test_undefined_variable_error_shows_name() {
    // The message names the variable, not a raw symbol id.
    eval_source("nonexistent-foo", |result| {
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
    });
}

// ============================================================================
// halt primitive — terminates the VM, cannot be tested in a script
// ============================================================================

#[test]
fn test_halt_returns_nil() {
    // (halt) with no args → NIL → Ok (clean exit)
    eval_source("(halt)", |result| {
        assert_eq!(result.unwrap(), Value::NIL);
    });
}

#[test]
fn test_halt_with_value_is_fatal() {
    // (halt <value>) → non-NIL → Err (fatal error, used for stack overflow etc.)
    eval_source("(halt 42)", |result| {
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("42"));
    });
}

#[test]
fn test_halt_no_args_stops_execution() {
    // (halt) stops execution and returns NIL
    eval_source("(begin (halt) 2)", |result| {
        assert_eq!(result.unwrap(), Value::NIL);
    });
}

#[test]
fn test_halt_with_value_stops_execution() {
    // (halt 1) stops execution with a fatal error (never reaches 2)
    eval_source("(begin (halt 1) 2)", |result| {
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("1"));
    });
}

#[test]
fn test_halt_with_value_in_function() {
    // (halt 99) inside a function → fatal error
    eval_source("(begin (def f (fn () (halt 99))) (f))", |result| {
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("99"));
    });
}
