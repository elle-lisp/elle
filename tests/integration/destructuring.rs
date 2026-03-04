// Integration tests for destructuring — Rust-only tests
//
// These tests check error messages or compile-time errors that require
// Rust-side string inspection. Behavioral tests are in tests/elle/destructuring.lisp.
use crate::common::eval_source;
use elle::Value;

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

// === CdrOrNil returns EMPTY_LIST for non-cons (#427) ===

#[test]
fn test_rest_list_empty_source_gives_empty_list() {
    // When destructuring an empty list, rest should be EMPTY_LIST, not NIL
    assert_eq!(
        eval_source("(begin (def (a & r) (list)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_shorter_source_gives_empty_list() {
    // When the list is shorter than the pattern, rest should be EMPTY_LIST
    assert_eq!(
        eval_source("(begin (def (a b & r) (list 1)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_on_non_list_gives_empty_list() {
    // When destructuring a non-list, rest should be EMPTY_LIST
    assert_eq!(
        eval_source("(begin (def (a & r) 42) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_truthiness() {
    // EMPTY_LIST is truthy; old NIL was falsy. This is the user-visible fix.
    assert_eq!(
        eval_source("(begin (def (a & r) (list)) (if r :truthy :falsy))").unwrap(),
        eval_source(":truthy").unwrap()
    );
}
