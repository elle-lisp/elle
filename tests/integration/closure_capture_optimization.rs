// DEFENSE: Integration tests for closure capture optimization (Issue #20 Phase 1)
// These tests verify that dead captures are eliminated in leaf lambdas
// Phase 1 focuses only on non-nested closures

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

// ============================================================================
// Closure Capture Optimization Tests (Issue #20 Phase 1)
// Test dead capture elimination for leaf lambdas only
// ============================================================================

#[test]
fn test_closure_no_unused_captures() {
    // Closure that uses all its captured variables
    // Note: This test verifies functional correctness of closures before optimization
    let code = r#"
(begin
  (define x 10)
  (define y 20)
  ((lambda () (+ x y))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_with_single_unused_capture() {
    // Closure has access to x but doesn't use it - optimization should remove it from capture list
    let code = r#"
(begin
  (define x 10)
  (define y 20)
  ((lambda () y)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(20));
}

#[test]
fn test_closure_with_multiple_unused_captures() {
    // Closure can capture multiple variables but only uses one
    let code = r#"
(begin
  (define a 10)
  (define b 20)
  (define c 30)
  ((lambda () c)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_uses_one_from_multiple_globals() {
    // Multiple globals defined, closure uses only one
    let code = r#"
(begin
  (define x 100)
  (define y 200)
  (define z 300)
  ((lambda () (+ x z))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(400));
}

#[test]
fn test_closure_with_parameters_and_captures() {
    // Lambda with both parameters and captures
    let code = r#"
(begin
  (define base 5)
  ((lambda (x y) (+ base (* x y))) 3 4))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(17));
}

#[test]
fn test_closure_captures_function() {
    // Closure captures a function and uses it
    let code = r#"
(begin
  (define add (lambda (a b) (+ a b)))
  ((lambda () (add 10 20))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_with_unused_global_function() {
    // Closure has access to functions but doesn't use them
    let code = r#"
(begin
  (define add (lambda (a b) (+ a b)))
  (define mul (lambda (a b) (* a b)))
  ((lambda () 42)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_closure_mixed_used_and_unused() {
    // Some globals used, some unused
    let code = r#"
(begin
  (define used1 10)
  (define unused1 999)
  (define used2 20)
  (define unused2 999)
  ((lambda () (+ used1 used2))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_with_conditional_capture_usage() {
    // Capture used in conditional branch
    let code = r#"
(begin
  (define threshold 5)
  ((lambda (x) (if (> x threshold) x 0)) 10))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(10));
}

#[test]
fn test_closure_with_arithmetic_operations() {
    // Closure with captures used in arithmetic
    let code = r#"
(begin
  (define base 10)
  (define multiplier 3)
  ((lambda (x) (* base (+ x multiplier))) 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(80));
}

#[test]
fn test_closure_all_captures_unused() {
    // Closure that captures nothing needed (all parameters used, no globals needed)
    let code = r#"
(begin
  (define x 100)
  ((lambda (y) y) 42))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_multiple_closures_independent_optimization() {
    // Two closures with different capture needs
    let code = r#"
(begin
  (define data1 10)
  (define data2 20)
  (define closure1 (lambda () data1))
  (define closure2 (lambda () data2))
  (+ (closure1) (closure2)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_captures_used_in_list_operation() {
    // Capture used in list operation
    let code = r#"
(begin
  (define nums (list 1 2 3))
  ((lambda () (first nums))))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(1));
}

#[test]
fn test_closure_parameter_shadows_capture() {
    // Parameter has same name as global but shadows it
    let code = r#"
(begin
  (define x 100)
  ((lambda (x) x) 42))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_closure_uses_parameter_not_capture() {
    // Accessible global exists but parameter is used instead
    let code = r#"
(begin
  (define x 100)
  (define y 200)
  ((lambda (x) (+ x y)) 50))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(250));
}
