// Tests for bracket syntax in special forms (issue #395).
//
// Error-message-checking tests that verify error messages contain
// specific strings. Tests that only check .is_ok() or .is_err()
// without message inspection have been migrated to tests/elle/brackets.lisp.

use crate::common::eval_source;

#[test]
fn match_arm_non_list_error() {
    let err = eval_source("(match 42 99)").unwrap_err();
    assert!(
        err.contains("got integer"),
        "Expected kind label in error, got: {err}"
    );
}

#[test]
fn fn_params_array_rejected() {
    let err = eval_source("(fn @[x] x)").unwrap_err();
    assert!(
        err.contains("(...)") || err.contains("[...]"),
        "Expected hint about parens or brackets, got: {err}"
    );
}

#[test]
fn let_bindings_array_rejected() {
    let err = eval_source("(let @[(x 1)] x)").unwrap_err();
    assert!(
        err.contains("(...)") || err.contains("[...]"),
        "Expected hint about parens or brackets, got: {err}"
    );
}
