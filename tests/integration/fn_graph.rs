use crate::common::eval_source;
use elle::Value;

#[test]
fn test_fn_dot_escape() {
    let result = eval_source(r#"(fn/dot-escape "Const { dst: Reg(0) }")"#).unwrap();
    assert_eq!(result, Value::string(r#"Const \{ dst: Reg(0) \}"#));
}

#[test]
fn test_fn_dot_escape_pipe() {
    let result = eval_source(r#"(fn/dot-escape "a|b")"#).unwrap();
    assert_eq!(result, Value::string(r#"a\|b"#));
}

#[test]
fn test_fn_dot_escape_angle_brackets() {
    let result = eval_source(r#"(fn/dot-escape "a<b>c")"#).unwrap();
    assert_eq!(result, Value::string(r#"a\<b\>c"#));
}

#[test]
fn test_fn_graph_returns_string() {
    let result = eval_source("(string? (fn/graph (fn/flow (fn (x) x))))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_starts_with_digraph() {
    let result =
        eval_source(r#"(string/starts-with? (fn/graph (fn/flow (fn (x) x))) "digraph {")"#)
            .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_ends_with_closing_brace() {
    let result = eval_source(
        r#"
        (let ((dot (fn/graph (fn/flow (fn (x) x)))))
          (string/ends-with? dot "}\n"))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_contains_block0() {
    let result =
        eval_source(r#"(string/contains? (fn/graph (fn/flow (fn (x) x))) "block0")"#).unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_contains_shape_record() {
    let result =
        eval_source(r#"(string/contains? (fn/graph (fn/flow (fn (x) x))) "shape=record")"#)
            .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_named_function_in_label() {
    // LirFunction.name is not currently set during lowering, so :name is nil
    // even for defn-defined closures. The graph label shows "anonymous".
    let result = eval_source(
        r#"
        (defn my-fn (x) (+ x 1))
        (string/contains? (fn/graph (fn/flow my-fn)) "anonymous")
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_branching_has_edges() {
    // An if-expression should produce "->" edge lines in the DOT output
    let result =
        eval_source(r#"(string/contains? (fn/graph (fn/flow (fn (x) (if x 1 2)))) "->")"#).unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_graph_shows_docstring_in_label() {
    let result = eval_source(
        r#"
        (defn my-fn (x) "Does stuff." (+ x 1))
        (string/contains? (fn/graph (fn/flow my-fn)) "Does stuff.")
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_save_graph_writes_file() {
    let path = std::env::temp_dir().join(format!("elle-test-graph-{}.dot", std::process::id()));
    let path = path.to_str().unwrap();
    let result = eval_source(&format!(
        r#"
        (defn test-fn (x) (+ x 1))
        (fn/save-graph test-fn "{path}")
        (string/starts-with? (slurp "{path}") "digraph {{")
        "#,
    ))
    .unwrap();
    let _ = std::fs::remove_file(path);
    assert_eq!(result, Value::TRUE);
}
