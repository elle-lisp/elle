// Regression tests for Bug 612: cond/match corrupt previously-evaluated
// arguments in variadic native function calls.
use crate::common::eval_source;
use elle::Value;

#[test]
fn test_cond_as_second_arg_to_path_join() {
    let result = eval_source(r#"(path/join "a" (cond (true "b")))"#).unwrap();
    assert_eq!(result, Value::string("a/b".to_string()));
}

#[test]
fn test_cond_as_second_arg_in_list() {
    let result = eval_source("(list 1 (cond (true 2)) 3)").unwrap();
    let expected = eval_source("(list 1 2 3)").unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_cond_as_third_arg_in_list() {
    let result = eval_source("(list 1 2 (cond (true 3)))").unwrap();
    let expected = eval_source("(list 1 2 3)").unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_cond_multi_clause_as_second_arg() {
    let result = eval_source(r#"(path/join "a" (cond (false "x") (true "b")))"#).unwrap();
    assert_eq!(result, Value::string("a/b".to_string()));
}

#[test]
fn test_match_as_second_arg_to_path_join() {
    let result = eval_source(r#"(path/join "a" (match 1 (1 "b") (_ "c")))"#).unwrap();
    assert_eq!(result, Value::string("a/b".to_string()));
}

#[test]
fn test_match_as_second_arg_in_list() {
    let result = eval_source("(list 1 (match 1 (1 2) (_ 0)) 3)").unwrap();
    let expected = eval_source("(list 1 2 3)").unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_match_wildcard_as_third_arg_in_list() {
    let result = eval_source("(list 1 2 (match 5 (1 99) (_ 3)))").unwrap();
    let expected = eval_source("(list 1 2 3)").unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_cond_as_first_arg_still_works() {
    let result = eval_source(r#"(path/join (cond (true "a")) "b")"#).unwrap();
    assert_eq!(result, Value::string("a/b".to_string()));
}

#[test]
fn test_if_as_second_arg_still_works() {
    let result = eval_source(r#"(path/join "a" (if true "b" "c"))"#).unwrap();
    assert_eq!(result, Value::string("a/b".to_string()));
}
