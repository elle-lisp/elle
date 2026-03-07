// Integration tests for destructuring — Rust-only tests
//
// These tests check error messages or compile-time errors that require
// Rust-side string inspection. Behavioral tests are in tests/elle/destructuring.lisp.
use crate::common::eval_source;

#[test]
fn test_def_destructured_bindings_are_immutable() {
    let result = eval_source("(begin (def (a b) (list 1 2)) (set a 10))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_variadic_arity_check_too_few() {
    // (fn (a b & rest) ...) requires at least 2 args
    assert!(eval_source("((fn (a b & rest) a) 1)").is_err());
}

#[test]
fn test_keys_duplicate_keys() {
    // Duplicate keyword keys → runtime error
    assert!(eval_source("((fn (a &keys opts) (get opts :x)) 1 :x 10 :x 20)").is_err());
}
