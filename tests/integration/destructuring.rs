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

// === Bug #408 regression: multi-block expressions in call arguments ===

#[test]
fn test_match_in_call_arg_with_trailing_args() {
    // Bug #408: match as call arg with more args after it
    let result =
        eval_source("(def f (fn [a b] a)) (f (match 42 (42 :found) (_ :nope)) :extra)").unwrap();
    assert_eq!(result, Value::keyword("found"));
}

#[test]
fn test_match_in_call_arg_not_first() {
    let result =
        eval_source("(def f (fn [a b] b)) (f :first (match 42 (42 :found) (_ :nope)))").unwrap();
    assert_eq!(result, Value::keyword("found"));
}

#[test]
fn test_match_in_call_arg_only_arg() {
    // This already works — regression guard
    let result = eval_source("(def f (fn [a] a)) (f (match 42 (42 :found) (_ :nope)))").unwrap();
    assert_eq!(result, Value::keyword("found"));
}

#[test]
fn test_cond_in_call_arg_with_trailing_args() {
    let result =
        eval_source("(def f (fn [a b] a)) (f (cond (true :yes) (false :no)) :extra)").unwrap();
    assert_eq!(result, Value::keyword("yes"));
}

#[test]
fn test_block_in_call_arg_with_trailing_args() {
    let result =
        eval_source("(def f (fn [a b] a)) (f (block :b (break :b :done)) :extra)").unwrap();
    assert_eq!(result, Value::keyword("done"));
}

#[test]
fn test_nested_match_in_call_arg() {
    let result = eval_source(
        "(def f (fn [a b] a)) (f (match 1 (1 (match 2 (2 :inner) (_ :no))) (_ :no)) :extra)",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("inner"));
}

#[test]
fn test_match_three_args() {
    let result =
        eval_source("(def f (fn [a b c] a)) (f (match 42 (42 :found) (_ :nope)) :b :c)").unwrap();
    assert_eq!(result, Value::keyword("found"));
}

#[test]
fn test_struct_destructure_rejects_non_key() {
    // Neither keyword nor quoted symbol — should error
    let result = eval_source("(def {42 v} {:x 1})");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("key must be a keyword or quoted symbol"));
}

#[test]
fn test_letrec_destructure_requires_body() {
    let result = eval_source("(letrec (((a b) (list 1 2))))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("requires bindings and body"));
}
