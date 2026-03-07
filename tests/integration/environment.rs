use crate::common::eval_source;

#[test]
fn test_environment_arity_error() {
    let result = eval_source("(environment 1)");
    assert!(result.is_err(), "environment should reject arguments");
    assert!(
        result.unwrap_err().contains("arity"),
        "error should mention arity"
    );
}
