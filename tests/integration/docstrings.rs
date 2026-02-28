use crate::common::eval_source;
use elle::Value;

#[test]
fn test_doc_returns_user_docstring() {
    let result = eval_source(
        r#"
        (def my-fn (fn (x) "Adds one to x" (+ x 1)))
        (doc "my-fn")
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::string("Adds one to x"));
}

#[test]
fn test_doc_returns_builtin_doc_when_no_user_doc() {
    let result = eval_source(r#"(doc "+")"#).unwrap();
    let s = result.with_string(|s| s.to_string()).unwrap();
    assert!(s.contains("+"));
}

#[test]
fn test_doc_returns_not_found_for_undocumented() {
    let result = eval_source(
        r#"
        (def x 42)
        (doc "x")
    "#,
    )
    .unwrap();
    let s = result.with_string(|s| s.to_string()).unwrap();
    assert!(s.contains("No documentation found"));
}

#[test]
fn test_doc_with_defn_macro() {
    let result = eval_source(
        r#"
        (defn greet (name) "Greets someone by name" (string/append "Hello, " name))
        (doc "greet")
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::string("Greets someone by name"));
}

#[test]
fn test_single_body_string_is_not_docstring() {
    let result = eval_source(
        r#"
        (def my-fn (fn () "hello"))
        (doc "my-fn")
    "#,
    )
    .unwrap();
    let s = result.with_string(|s| s.to_string()).unwrap();
    assert!(s.contains("No documentation found"));
}
