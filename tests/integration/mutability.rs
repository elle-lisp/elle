use crate::common::eval_source;
use elle::value::Value;

// ── Mutability tests ──────────────────────────────────────────────────────

#[test]
fn test_immutable_by_default_let() {
    // Let bindings are immutable by default; assign should fail
    let result = eval_source("(let [x 5] (assign x 10) x)");
    assert!(
        result.is_err(),
        "assigning to an immutable let binding should fail"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("immutable"),
        "error should mention immutability: {err}"
    );
}

#[test]
fn test_mutable_let_with_at() {
    // @x opt-in to mutability
    let result = eval_source("(let [@x 5] (assign x 10) x)").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_immutable_by_default_def() {
    let result = eval_source("(def x 5) (assign x 10) x");
    assert!(
        result.is_err(),
        "assigning to an immutable def binding should fail"
    );
}

#[test]
fn test_mutable_def_with_at() {
    let result = eval_source("(var @x 5) (assign x 10) x").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_immutable_lambda_param() {
    // Lambda parameters are immutable by default
    let result = eval_source("(defn f [x] (assign x 10) x) (f 5)");
    assert!(
        result.is_err(),
        "assigning to an immutable lambda parameter should fail"
    );
}

#[test]
fn test_mutable_lambda_param_with_at() {
    let result = eval_source("(defn f [@x] (assign x 10) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(10));
}

// ── silent! ──────────────────────────────────────────────────────────────

#[test]
fn test_silence_assert_passes_for_silent_fn() {
    // Use identity — truly silent (no arithmetic that can emit :error)
    let result = eval_source("(defn f [x] (silent!) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_silence_assert_fails_for_yielding_fn() {
    let result = eval_source("(defn f [x] (silent!) (emit :yield x)) (f 5)");
    assert!(result.is_err(), "silent! should fail for yielding fn");
    let err = result.unwrap_err();
    assert!(
        err.contains("silent! assertion failed"),
        "error: {err}"
    );
}

#[test]
fn test_silence_assert_outside_fn() {
    let result = eval_source("(silent!)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("must appear inside a function body"),
        "error: {err}"
    );
}

// ── immutable! ───────────────────────────────────────────────────────────

#[test]
fn test_immutable_assert_passes() {
    let result = eval_source("(defn f [@x] (immutable! x) x) (f 5)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_immutable_assert_fails() {
    let result = eval_source("(defn f [@x] (immutable! x) (assign x 10) x) (f 5)");
    assert!(
        result.is_err(),
        "immutable! should fail when binding is assigned"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("immutable! assertion failed"),
        "error: {err}"
    );
}

// ── numeric! ─────────────────────────────────────────────────────────────

#[test]
fn test_numeric_assert_passes_for_pure_arithmetic() {
    let result = eval_source("(defn f [x y] (numeric!) (%add x y)) (f 3 4)").unwrap();
    assert_eq!(result, Value::int(7));
}

#[test]
fn test_numeric_assert_fails_for_call() {
    // A function that calls another function is not GPU-eligible
    let result = eval_source(
        "(defn helper [x] x) (defn f [x] (numeric!) (helper x)) (f 5)",
    );
    assert!(
        result.is_err(),
        "numeric! should fail for non-GPU-eligible fn"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("numeric! assertion failed"),
        "error: {err}"
    );
}
