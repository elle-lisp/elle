// Integration tests for @-mutability annotations and compile-time assertions.
use crate::common::eval_source;
use elle::Value;

// ── def @ ────────────────────────────────────────────────────────────

#[test]
fn test_def_immutable_rejects_assign() {
    let result = eval_source("(def n 3) (assign n 4)");
    assert!(result.is_err(), "def without @ should reject assign");
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot assign immutable binding"),
        "error: {err}"
    );
}

#[test]
fn test_def_at_allows_assign() {
    let result = eval_source("(def @n 3) (assign n (inc n)) n").unwrap();
    assert_eq!(result, Value::int(4));
}

// ── let @ ────────────────────────────────────────────────────────────

#[test]
fn test_let_immutable_rejects_assign() {
    let result = eval_source("(let [x 1] (assign x 2))");
    assert!(result.is_err(), "let without @ should reject assign");
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot assign immutable binding"),
        "error: {err}"
    );
}

#[test]
fn test_let_at_allows_assign() {
    let result = eval_source("(let [@x 1] (assign x 2) x)").unwrap();
    assert_eq!(result, Value::int(2));
}

// ── param @ ──────────────────────────────────────────────────────────

#[test]
fn test_param_immutable_rejects_assign() {
    let result = eval_source("(defn f [x] (assign x 10)) (f 5)");
    assert!(result.is_err(), "param without @ should reject assign");
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot assign immutable binding"),
        "error: {err}"
    );
}

#[test]
fn test_param_at_allows_assign() {
    let result = eval_source("(defn f [@x] (assign x 10) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(10));
}

// ── assert-silent ─────────────────────────────────────────────────────────

#[test]
fn test_silence_assert_passes_for_silent_fn() {
    // Use identity — truly silent (no arithmetic that can emit :error)
    let result = eval_source("(defn f [x] (assert-silent) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_silence_assert_fails_for_yielding_fn() {
    let result = eval_source("(defn f [x] (assert-silent) (emit :yield x)) (f 5)");
    assert!(result.is_err(), "assert-silent should fail for yielding fn");
    let err = result.unwrap_err();
    assert!(
        err.contains("assert-silent assertion failed"),
        "error: {err}"
    );
}

#[test]
fn test_silence_assert_outside_fn() {
    let result = eval_source("(assert-silent)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("must appear inside a function body"),
        "error: {err}"
    );
}

// ── assert-immutable ───────────────────────────────────────────────────────

#[test]
fn test_immutable_assert_passes() {
    let result = eval_source("(defn f [@x] (assert-immutable x) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_immutable_assert_fails() {
    let result = eval_source("(defn f [@x] (assert-immutable x) (assign x 10) x) (f 5)");
    assert!(
        result.is_err(),
        "assert-immutable should fail when binding is assigned"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("assert-immutable assertion failed"),
        "error: {err}"
    );
}

// ── assert-numeric ─────────────────────────────────────────────────────────

#[test]
fn test_numeric_assert_passes_for_pure_arithmetic() {
    let result = eval_source("(defn f [x y] (assert-numeric) (+ x y)) (f 3 4)").unwrap();
    assert_eq!(result, Value::int(7));
}

#[test]
fn test_numeric_assert_fails_for_call() {
    // A function that calls another function is not GPU-eligible
    let result = eval_source(
        "(defn helper [x] x) (defn f [x] (assert-numeric) (helper x)) (f 5)",
    );
    assert!(
        result.is_err(),
        "assert-numeric should fail for non-GPU-eligible fn"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("assert-numeric assertion failed"),
        "error: {err}"
    );
}

// ── epoch: var → def @ ──────────────────────────────────────────────

#[test]
fn test_var_still_works() {
    // var continues to work for backward compat
    let result = eval_source("(var v 10) (assign v 20) v").unwrap();
    assert_eq!(result, Value::int(20));
}

// ── destructure @ ────────────────────────────────────────────────────

#[test]
fn test_destructure_def_at() {
    let result = eval_source("(def [@a b] [1 2]) (assign a 10) (+ a b)").unwrap();
    assert_eq!(result, Value::int(12));
}

#[test]
fn test_destructure_def_immutable_rejects() {
    let result = eval_source("(def [a b] [1 2]) (assign a 10)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot assign immutable binding"),
        "error: {err}"
    );
}

#[test]
fn test_destructure_let_at() {
    let result =
        eval_source("(let [[@x y] [1 2]] (assign x 99) (+ x y))").unwrap();
    assert_eq!(result, Value::int(101));
}
