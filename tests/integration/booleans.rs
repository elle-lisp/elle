// Integration tests for boolean literals (true/false)
use crate::common::eval_source;
use elle::Value;

#[test]
fn test_true_literal() {
    assert_eq!(eval_source("true").unwrap(), Value::TRUE);
}

#[test]
fn test_false_literal() {
    assert_eq!(eval_source("false").unwrap(), Value::FALSE);
}

#[test]
fn test_if_true() {
    assert_eq!(eval_source("(if true 1 2)").unwrap(), Value::int(1));
}

#[test]
fn test_if_false() {
    assert_eq!(eval_source("(if false 1 2)").unwrap(), Value::int(2));
}

#[test]
fn test_boolean_predicate_true() {
    assert_eq!(eval_source("(boolean? true)").unwrap(), Value::TRUE);
}

#[test]
fn test_boolean_predicate_false() {
    assert_eq!(eval_source("(boolean? false)").unwrap(), Value::TRUE);
}

#[test]
fn test_match_false_with_word_pattern() {
    assert_eq!(
        eval_source(r#"(match false (true "yes") (false "no"))"#).unwrap(),
        Value::string("no"),
    );
}

#[test]
fn test_match_cross_form() {
    assert_eq!(
        eval_source(r#"(match true (true "yes") (false "no"))"#).unwrap(),
        Value::string("yes"),
    );
}

#[test]
fn test_quoted_true_is_boolean() {
    // 'true produces Value::TRUE (consistent with 'nil → Value::NIL)
    assert_eq!(eval_source("'true").unwrap(), Value::TRUE);
}

#[test]
fn test_read_true() {
    assert_eq!(eval_source(r#"(read "true")"#).unwrap(), Value::TRUE);
}

#[test]
fn test_string_true() {
    assert_eq!(eval_source("(string true)").unwrap(), Value::string("true"),);
}

#[test]
fn test_display_roundtrip() {
    // read(string(true)) → Value::TRUE
    assert_eq!(eval_source("(read (string true))").unwrap(), Value::TRUE);
}
