/// Tests for peephole optimizations (Issue #170)
use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

#[test]
fn test_length_zero_optimization_empty_list() {
    // (= (length '()) 0) should return true
    assert_eq!(eval("(= (length '()) 0)").unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_optimization_non_empty_list() {
    // (= (length '(1 2 3)) 0) should return false
    assert_eq!(eval("(= (length '(1 2 3)) 0)").unwrap(), Value::bool(false));
}

#[test]
fn test_length_zero_optimization_reversed_empty() {
    // (= 0 (length '())) should also work
    assert_eq!(eval("(= 0 (length '()))").unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_optimization_reversed_non_empty() {
    assert_eq!(eval("(= 0 (length '(1 2)))").unwrap(), Value::bool(false));
}

#[test]
fn test_length_zero_optimization_vector_empty() {
    assert_eq!(eval("(= (length []) 0)").unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_optimization_vector_non_empty() {
    assert_eq!(eval("(= (length [1 2 3]) 0)").unwrap(), Value::bool(false));
}

#[test]
fn test_length_zero_optimization_string_empty() {
    assert_eq!(eval("(= (length \"\") 0)").unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_optimization_string_non_empty() {
    assert_eq!(
        eval("(= (length \"hello\") 0)").unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_length_zero_in_conditional() {
    // Test that optimization works in conditionals
    let result = eval("(if (= (length '()) 0) \"empty\" \"not empty\")");
    assert_eq!(result.unwrap(), Value::string("empty"));
}

#[test]
fn test_length_zero_in_conditional_non_empty() {
    // Test that optimization works in conditionals
    let result = eval("(if (= (length '(a b c)) 0) \"empty\" \"not empty\")");
    assert_eq!(result.unwrap(), Value::string("not empty"));
}

#[test]
fn test_length_zero_in_recursion() {
    // Test in a recursive context (the main use case)
    let code = r#"
        (begin
            (define count-elements
              (fn (lst acc)
                (if (= (length lst) 0)
                    acc
                    (count-elements (rest lst) (+ acc 1)))))
            (count-elements '(a b c d e) 0))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_length_zero_in_recursion_empty() {
    // Test in a recursive context with empty list
    let code = r#"
        (begin
            (define count-elements
              (fn (lst acc)
                (if (= (length lst) 0)
                    acc
                    (count-elements (rest lst) (+ acc 1)))))
            (count-elements '() 0))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(0));
}

#[test]
fn test_non_zero_comparison_not_optimized() {
    // (= (length x) 1) should NOT be optimized (different semantics)
    assert_eq!(eval("(= (length '(a)) 1)").unwrap(), Value::bool(true));
    assert_eq!(eval("(= (length '()) 1)").unwrap(), Value::bool(false));
}

#[test]
fn test_length_greater_than_zero_not_optimized() {
    // (> (length x) 0) should NOT be optimized
    assert_eq!(eval("(> (length '(a b c)) 0)").unwrap(), Value::bool(true));
    assert_eq!(eval("(> (length '()) 0)").unwrap(), Value::bool(false));
}

#[test]
fn test_length_zero_with_variable() {
    // Test with a variable
    let code = r#"
        (begin
            (define my-list '(1 2 3))
            (= (length my-list) 0))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(false));
}

#[test]
fn test_length_zero_with_empty_variable() {
    // Test with an empty variable
    let code = r#"
        (begin
            (define my-list '())
            (= (length my-list) 0))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_in_and_expression() {
    // Test optimization in AND expression
    let code = r#"
        (and (= (length '()) 0) (= 1 1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_in_or_expression() {
    // Test optimization in OR expression
    let code = r#"
        (or (= (length '(a)) 0) (= 1 1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_in_nested_if() {
    // Test optimization in nested if expressions
    let code = r#"
        (if (= (length '()) 0)
            (if (= (length '(a)) 0) "both empty" "first empty")
            "first not empty")
    "#;
    assert_eq!(eval(code).unwrap(), Value::string("first empty"));
}

#[test]
fn test_length_zero_in_let_binding() {
    // Test optimization in let binding
    let code = r#"
        (let ((is-empty (= (length '()) 0)))
          is-empty)
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_in_lambda() {
    // Test optimization in lambda
    let code = r#"
        (begin
            (define check-empty
              (fn (lst)
                (= (length lst) 0)))
            (check-empty '()))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_length_zero_in_lambda_non_empty() {
    // Test optimization in lambda with non-empty list
    let code = r#"
        (begin
            (define check-empty
              (fn (lst)
                (= (length lst) 0)))
            (check-empty '(a b c)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(false));
}
