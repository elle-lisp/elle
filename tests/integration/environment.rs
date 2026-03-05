use crate::common::eval_source;
use elle::Value;

#[test]
fn test_environment_returns_struct() {
    let result = eval_source("(struct? (environment))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_environment_contains_primitives() {
    // The + primitive should be in the environment as a keyword key
    let result = eval_source("(not (nil? (get (environment) :+)))").unwrap();
    assert_eq!(result, Value::TRUE, ":+ should be in environment");
}

#[test]
fn test_environment_contains_user_defined_global() {
    let result = eval_source("(def my-val 42) (get (environment) :my-val)").unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_environment_reflects_mutation() {
    let result = eval_source("(var x 1) (set x 2) (get (environment) :x)").unwrap();
    assert_eq!(result, Value::int(2));
}

#[test]
fn test_environment_via_vm_query() {
    let result = eval_source(r#"(struct? (vm/query "environment" nil))"#).unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_environment_excludes_undefined() {
    // Globals that were never assigned should not appear
    let result = eval_source("(nil? (get (environment) :__nonexistent_symbol_42__))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_environment_arity_error() {
    let result = eval_source("(environment 1)");
    assert!(result.is_err(), "environment should reject arguments");
    assert!(
        result.unwrap_err().contains("arity"),
        "error should mention arity"
    );
}
