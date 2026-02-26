// Tests for bracket-vs-parenthesis error messages (issue #349).
//
// Verifies that using [...] where (...) is expected produces a helpful
// error mentioning "not brackets" rather than the old generic "must be
// a list" message.

use crate::common::eval_source;

#[test]
fn match_arm_bracket_error() {
    let err = eval_source("(match 42 [42 \"yes\"])").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}

#[test]
fn match_arm_non_list_error() {
    let err = eval_source("(match 42 99)").unwrap_err();
    assert!(
        err.contains("got integer"),
        "Expected kind label in error, got: {err}"
    );
}

#[test]
fn cond_clause_bracket_error() {
    let err = eval_source("(cond [#t 1])").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}

#[test]
fn let_bindings_bracket_error() {
    let err = eval_source("(let [(x 1)] x)").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}

#[test]
fn fn_params_bracket_error() {
    let err = eval_source("(fn [x] x)").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}

#[test]
fn defmacro_params_bracket_error() {
    let err = eval_source("(defmacro foo [x] x)").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}

#[test]
fn letrec_bindings_bracket_error() {
    let err = eval_source("(letrec [(f (fn (x) x))] (f 1))").unwrap_err();
    assert!(
        err.contains("not brackets"),
        "Expected hint about brackets, got: {err}"
    );
}
