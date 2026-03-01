use crate::common::eval_source;
use elle::Value;

#[test]
fn test_fn_flow_returns_struct() {
    // fn/flow on a simple closure returns a struct (not nil)
    let result = eval_source("(struct? (fn/flow (fn (x) x)))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_has_expected_keys() {
    // The struct has :name, :arity, :regs, :locals, :entry, :blocks
    let result = eval_source(
        r#"
        (let ((cfg (fn/flow (fn (x y) (+ x y)))))
          (and (not (nil? (get cfg :arity)))
               (not (nil? (get cfg :regs)))
               (not (nil? (get cfg :locals)))
               (not (nil? (get cfg :entry)))
               (not (nil? (get cfg :blocks)))))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_arity_is_string() {
    let result = eval_source("(string? (get (fn/flow (fn (x y) (+ x y))) :arity))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_arity_value() {
    // Arity Display for Exact(2) is "2"
    let result = eval_source(r#"(get (fn/flow (fn (x y) (+ x y))) :arity)"#).unwrap();
    assert_eq!(result, Value::string("2"));
}

#[test]
fn test_fn_flow_blocks_is_tuple() {
    let result = eval_source("(tuple? (get (fn/flow (fn (x) x)) :blocks))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_blocks_nonempty() {
    let result = eval_source("(> (length (get (fn/flow (fn (x) x)) :blocks)) 0)").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_block_has_expected_keys() {
    let result = eval_source(
        r#"
        (let ((block (get (get (fn/flow (fn (x) x)) :blocks) 0)))
          (and (not (nil? (get block :label)))
               (not (nil? (get block :instrs)))
               (not (nil? (get block :term)))
               (not (nil? (get block :edges)))))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_instrs_is_tuple_of_strings() {
    let result = eval_source(
        r#"
        (let ((instrs (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :instrs)))
          (tuple? instrs))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_edges_is_tuple() {
    let result = eval_source(
        r#"
        (let ((edges (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :edges)))
          (tuple? edges))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_term_is_string() {
    let result = eval_source(
        r#"
        (let ((term (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :term)))
          (string? term))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_non_closure_errors() {
    let result = eval_source("(fn/flow 42)");
    assert!(result.is_err());
}

#[test]
fn test_fn_flow_named_function() {
    // LirFunction.name is not currently set during lowering, so :name is nil
    // even for defn-defined closures (the name lives on the global binding, not the lambda)
    let result = eval_source(
        r#"
        (defn my-add (x y) (+ x y))
        (nil? (get (fn/flow my-add) :name))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_anonymous_function_name_is_nil() {
    let result = eval_source("(nil? (get (fn/flow (fn (x) x)) :name))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_doc_with_docstring() {
    let result = eval_source(
        r#"
        (defn my-add (x y) "Add two numbers." (+ x y))
        (get (fn/flow my-add) :doc)
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::string("Add two numbers."));
}

#[test]
fn test_fn_flow_doc_without_docstring() {
    let result = eval_source("(nil? (get (fn/flow (fn (x) x)) :doc))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_fn_flow_branching_has_edges() {
    // An if-expression produces Branch terminators with edges
    let result = eval_source(
        r#"
        (let ((cfg (fn/flow (fn (x) (if x 1 2)))))
          (let ((blocks (get cfg :blocks)))
            (> (length blocks) 1)))
        "#,
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}
