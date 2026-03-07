use crate::common::eval_source;

// ── Error messages ──────────────────────────────────────────────

#[test]
fn test_rejected_type_still_errors() {
    let result = eval_source(r#"(let ((t @{})) (put t @[1 2] :val))"#);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("expected hashable value"),
        "Error should mention 'expected hashable value', got: {}",
        err
    );
}
