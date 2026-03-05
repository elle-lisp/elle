// Parameter type tests
//
// Tests for Racket-style dynamic parameters that require Rust type inspection.
// Most behavioral tests are in tests/elle/parameters.lisp.

use crate::common::eval_source;

#[test]
fn test_make_parameter_returns_parameter() {
    let result = eval_source("(make-parameter 42)").unwrap();
    assert!(result.is_parameter());
}

#[test]
fn test_parameter_type_of() {
    let result = eval_source("(type (make-parameter 0))").unwrap();
    assert_eq!(result.as_keyword_name(), Some("parameter"));
}

#[test]
fn test_parameter_call_with_args_errors() {
    let result = eval_source("((make-parameter 42) 1)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("expected 0 arguments"), "got: {}", err);
}

#[test]
fn test_parameterize_non_parameter_errors() {
    let result = eval_source("(parameterize ((42 1)) 0)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not a parameter"),
        "expected 'not a parameter' error, got: {}",
        err
    );
}
