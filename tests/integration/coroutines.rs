// Integration tests for colorless coroutines (issue #236)
//
// This file contains only tests that verify error message content.
// Behavioral tests have been migrated to tests/elle/coroutines.lisp.

use crate::common::eval_source;

#[test]
fn test_resume_done_coroutine_fails() {
    // Resuming a done coroutine should error
    let result = eval_source(
        r#"
        (var co (make-coroutine (fn () 42)))
        (coro/resume co)
        (coro/resume co)
        "#,
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("cannot resume completed coroutine"));
}
