use crate::common::eval_source;

// ── Error cases (compile-time and error-message checks) ──────────

#[test]
fn test_splice_invalid_context() {
    // Splice at top level should error
    let result = eval_source(";@[1 2 3]");
    assert!(result.is_err(), "splice at top level should error");
    assert!(
        result.unwrap_err().contains("splice"),
        "error should mention splice"
    );
}

#[test]
fn test_splice_wrong_type() {
    // Splicing a non-indexed type should error at runtime
    let result = eval_source("(+ ;42)");
    assert!(result.is_err(), "splicing an integer should error");
}

#[test]
fn test_splice_in_struct_rejected() {
    // Splice in struct literal should be rejected at compile time
    let result = eval_source("{:a 1 ;@[:b 2]}");
    assert!(result.is_err(), "splice in struct should error");
}

#[test]
fn test_splice_in_table_rejected() {
    // Splice in table literal should be rejected at compile time
    let result = eval_source("@{:a 1 ;@[:b 2]}");
    assert!(result.is_err(), "splice in table should error");
}

#[test]
fn test_nested_splice() {
    // ;;@[1 2] is splice-of-splice. The inner splice in a non-call context
    // should error at compile time.
    let result = eval_source(";;@[1 2]");
    assert!(result.is_err(), "nested splice should error");
}

#[test]
fn test_splice_in_let_binding() {
    // Splice in a let binding pattern position should be rejected.
    // (let ((;@[a b] @[1 2])) a) should error at compile time.
    let result = eval_source("(let ((;@[a b] @[1 2])) a)");
    assert!(
        result.is_err(),
        "splice in let binding pattern should error"
    );
}
