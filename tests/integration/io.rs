use crate::common::eval_source;

#[test]
fn test_io_request_predicate_false_on_int() {
    let result = eval_source("(io-request? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_request_predicate_false_on_string() {
    let result = eval_source("(io-request? \"hello\")").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_backend_predicate_false_on_int() {
    let result = eval_source("(io-backend? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}
