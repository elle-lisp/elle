// Tests for bracket syntax in special forms (issue #395).
//
// Verifies that [...] (SyntaxKind::Tuple) is accepted in structural
// positions: params, bindings, clauses, match arms. @[...] (Array)
// is still rejected.

use crate::common::eval_source;
use elle::Value;

#[test]
fn fn_params_bracket() {
    assert_eq!(eval_source("((fn [x] x) 42)").unwrap(), Value::int(42));
}

#[test]
fn fn_params_bracket_multi() {
    assert_eq!(
        eval_source("((fn [x y] (+ x y)) 1 2)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn fn_params_bracket_rest() {
    assert_eq!(
        eval_source("((fn [x & xs] x) 1 2 3)").unwrap(),
        Value::int(1)
    );
}

#[test]
fn let_bindings_bracket_outer() {
    assert_eq!(eval_source("(let [(x 1)] x)").unwrap(), Value::int(1));
}

#[test]
fn let_binding_pair_bracket() {
    assert_eq!(eval_source("(let ([x 1]) x)").unwrap(), Value::int(1));
}

#[test]
fn let_bindings_bracket_both() {
    assert_eq!(eval_source("(let [[x 1]] x)").unwrap(), Value::int(1));
}

#[test]
fn letrec_bindings_bracket() {
    assert_eq!(
        eval_source("(letrec [(f (fn (x) x))] (f 1))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn letrec_binding_pair_bracket() {
    assert_eq!(
        eval_source("(letrec ([f (fn (x) x)]) (f 1))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn cond_clause_bracket() {
    assert_eq!(eval_source("(cond [true 42])").unwrap(), Value::int(42));
}

#[test]
fn cond_clause_bracket_else() {
    assert_eq!(
        eval_source("(cond [false 1] [else 42])").unwrap(),
        Value::int(42)
    );
}

#[test]
fn match_arm_bracket() {
    assert_eq!(
        eval_source("(match 42 [42 \"yes\"])").unwrap(),
        Value::string("yes")
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
fn defmacro_params_bracket() {
    assert_eq!(
        eval_source("(defmacro id [x] x) (id 7)").unwrap(),
        Value::int(7)
    );
}

#[test]
fn defn_params_bracket() {
    assert_eq!(
        eval_source("(defn f [x] x) (f 99)").unwrap(),
        Value::int(99)
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
