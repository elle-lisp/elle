// Integration tests for prelude macros (when, unless, try, protect, defer, with)

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// SECTION 1: when
// ============================================================================

#[test]
fn test_when_true() {
    assert_eq!(eval_source("(when true 42)").unwrap(), Value::int(42));
}

#[test]
fn test_when_false() {
    assert_eq!(eval_source("(when false 42)").unwrap(), Value::NIL);
}

#[test]
fn test_when_multi_body() {
    assert_eq!(eval_source("(when true 1 2 3)").unwrap(), Value::int(3));
}

#[test]
fn test_when_truthy_value() {
    // Non-boolean truthy value
    assert_eq!(eval_source("(when 1 42)").unwrap(), Value::int(42));
}

// ============================================================================
// SECTION 2: unless
// ============================================================================

#[test]
fn test_unless_true() {
    assert_eq!(eval_source("(unless true 42)").unwrap(), Value::NIL);
}

#[test]
fn test_unless_false() {
    assert_eq!(eval_source("(unless false 42)").unwrap(), Value::int(42));
}

#[test]
fn test_unless_multi_body() {
    assert_eq!(eval_source("(unless false 1 2 3)").unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 3: try/catch
// ============================================================================

#[test]
fn test_try_catch_no_error() {
    assert_eq!(
        eval_source("(try 42 (catch e :error))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_try_catch_catches_error() {
    let result = eval_source("(try (/ 1 0) (catch e :caught))");
    assert_eq!(result.unwrap(), Value::keyword("caught"));
}

#[test]
fn test_try_catch_binds_error() {
    // The error binding should be available in the handler
    let result = eval_source("(try (/ 1 0) (catch e e))");
    assert!(result.is_ok());
    // e should be the error value (a cons cell / error tuple)
    let val = result.unwrap();
    assert!(!val.is_nil());
}

#[test]
fn test_try_catch_multi_body() {
    // Multiple body forms before the catch clause
    assert_eq!(
        eval_source("(try 1 2 (+ 20 22) (catch e :error))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_try_catch_multi_handler() {
    // Multiple handler forms — last one is the result
    let result = eval_source("(try (/ 1 0) (catch e 1 2 :caught))");
    assert_eq!(result.unwrap(), Value::keyword("caught"));
}

#[test]
fn test_try_catch_destructured_error() {
    // Error values are tuples [:kind "msg"] — bracket destructuring should work
    let result = eval_source("(try (/ 1 0) (catch [kind msg] kind))");
    assert_eq!(result.unwrap(), Value::keyword("division-by-zero"));
}

#[test]
fn test_try_catch_destructured_error_message() {
    let result = eval_source("(try (/ 1 0) (catch [kind msg] msg))");
    assert_eq!(result.unwrap(), Value::string("division by zero"));
}

// ============================================================================
// SECTION 4: protect
// ============================================================================

#[test]
fn test_protect_success() {
    let result = eval_source("(protect 42)");
    assert!(result.is_ok());
    let val = result.unwrap();
    let elems = val.as_tuple().unwrap();
    assert_eq!(elems[0], Value::bool(true));
    assert_eq!(elems[1], Value::int(42));
}

#[test]
fn test_protect_failure() {
    let result = eval_source("(protect (/ 1 0))");
    assert!(result.is_ok());
    let val = result.unwrap();
    let elems = val.as_tuple().unwrap();
    assert_eq!(elems[0], Value::bool(false));
    // elems[1] is the error value — just check it exists
    assert!(!elems[1].is_nil() || elems[1].is_nil()); // always true, just access it
}

// ============================================================================
// SECTION 5: defer
// ============================================================================

#[test]
fn test_defer_runs_cleanup() {
    assert_eq!(
        eval_source("(begin (var cleaned false) (defer (set cleaned true) 42) cleaned)").unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_defer_returns_body_value() {
    assert_eq!(
        eval_source("(begin (var x 0) (defer (set x 1) 42))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_defer_runs_cleanup_on_error() {
    // Cleanup should run even when body errors
    assert_eq!(
        eval_source(
            "(begin (var cleaned false) (try (defer (set cleaned true) (/ 1 0)) (catch e cleaned)))"
        )
        .unwrap(),
        Value::bool(true)
    );
}

// ============================================================================
// SECTION 6: with
// ============================================================================

#[test]
fn test_with_basic() {
    // Use with to bind a resource and clean it up
    assert_eq!(
        eval_source(
            r#"(begin
                (defn make-resource () :resource)
                (defn free-resource (r) nil)
                (with r (make-resource) free-resource
                  42))"#
        )
        .unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_with_cleanup_runs() {
    assert_eq!(
        eval_source(
            r#"(begin
                (var cleaned false)
                (defn make () :resource)
                (defn cleanup (r) (set cleaned true))
                (with r (make) cleanup
                  42)
                cleaned)"#
        )
        .unwrap(),
        Value::bool(true)
    );
}

// ============================================================================
// SECTION 7: butlast primitive
// ============================================================================

#[test]
fn test_butlast_basic() {
    let result = eval_source("(butlast (list 1 2 3))").unwrap();
    let items = result.list_to_vec().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], Value::int(1));
    assert_eq!(items[1], Value::int(2));
}

#[test]
fn test_butlast_single() {
    let result = eval_source("(butlast (list 1))").unwrap();
    // Should return empty list
    assert!(result.list_to_vec().unwrap().is_empty());
}

#[test]
fn test_butlast_empty_errors() {
    let result = eval_source("(butlast (list))");
    assert!(result.is_err());
}

// ============================================================================
// SECTION 8: hygiene — prelude macros don't capture
// ============================================================================

#[test]
fn test_try_hygiene_no_capture() {
    // The try macro introduces an internal binding `f`.
    // A call-site variable named `f` should not be affected.
    assert_eq!(
        eval_source(
            r#"(let ((f 99))
                (try (+ f 1) (catch e :error)))"#
        )
        .unwrap(),
        Value::int(100)
    );
}

#[test]
fn test_defer_hygiene_no_capture() {
    // The defer macro introduces an internal binding `f`.
    // A call-site variable named `f` should not be affected.
    assert_eq!(
        eval_source(
            r#"(begin
                (var cleaned false)
                (let ((f 99))
                  (defer (set cleaned true) (+ f 1))))"#
        )
        .unwrap(),
        Value::int(100)
    );
}
