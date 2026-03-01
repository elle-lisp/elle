use crate::common::eval_source;
use elle::Value;

// ── Fiber keys ──────────────────────────────────────────────────

#[test]
fn test_fiber_as_table_key() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((t @{}))
               (put t f :running)
               (get t f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("running"));
}

#[test]
fn test_fiber_key_overwrites_same_key() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((t @{}))
               (put t f 1)
               (put t f 2)
               (get t f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(2));
}

#[test]
fn test_different_fibers_are_different_keys() {
    let result = eval_source(
        r#"(let ((f1 (fiber/new (fn () 1) 0))
                  (f2 (fiber/new (fn () 2) 0)))
             (let ((t @{}))
               (put t f1 :a)
               (put t f2 :b)
               (get t f1)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("a"));
}

#[test]
fn test_has_key_with_fiber() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((t @{}))
               (put t f 1)
               (has-key? t f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_del_with_fiber_key() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((t @{}))
               (put t f 1)
               (del t f)
               (has-key? t f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::FALSE);
}

#[test]
fn test_keys_roundtrip_identity_fiber() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((t @{}))
               (put t f 1)
               (eq? (first (keys t)) f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fiber_as_struct_key() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0)))
             (let ((s (struct f :val)))
               (get s f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("val"));
}

// ── Closure keys ────────────────────────────────────────────────

#[test]
fn test_closure_as_table_key() {
    let result = eval_source(
        r#"(let ((c (fn () 1)))
             (let ((t @{}))
               (put t c :meta)
               (get t c)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("meta"));
}

#[test]
fn test_closure_key_overwrites_same_key() {
    let result = eval_source(
        r#"(let ((c (fn () 1)))
             (let ((t @{}))
               (put t c 1)
               (put t c 2)
               (get t c)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(2));
}

#[test]
fn test_different_closures_are_different_keys() {
    let result = eval_source(
        r#"(let ((c1 (fn () 1))
                  (c2 (fn () 2)))
             (let ((t @{}))
               (put t c1 :a)
               (put t c2 :b)
               (get t c1)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("a"));
}

#[test]
fn test_keys_roundtrip_identity_closure() {
    let result = eval_source(
        r#"(let ((c (fn () 1)))
             (let ((t @{}))
               (put t c 1)
               (eq? (first (keys t)) c)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_closure_as_struct_key() {
    let result = eval_source(
        r#"(let ((c (fn () 1)))
             (let ((s (struct c :val)))
               (get s c)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("val"));
}

// ── Mixed keys ──────────────────────────────────────────────────

#[test]
fn test_mixed_keys_fiber_closure_keyword() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0))
                  (c (fn () 2)))
             (let ((t @{}))
               (put t :name "proc")
               (put t f :fiber-data)
               (put t c :closure-data)
               (get t f)))"#,
    )
    .unwrap();
    assert_eq!(result, Value::keyword("fiber-data"));
}

#[test]
fn test_fiber_and_closure_are_different_keys() {
    let result = eval_source(
        r#"(let ((f (fiber/new (fn () 1) 0))
                  (c (fn () 2)))
             (let ((t @{}))
               (put t f :fib)
               (put t c :clo)
               (list (get t f) (get t c))))"#,
    )
    .unwrap();
    // Result is a list (:fib :clo)
    let cons = result.as_cons().expect("expected cons");
    assert_eq!(cons.first, Value::keyword("fib"));
    let cons2 = cons.rest.as_cons().expect("expected cons");
    assert_eq!(cons2.first, Value::keyword("clo"));
}

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
