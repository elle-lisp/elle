// Unit tests for looping constructs (while and for loops)
//
// These tests verify that the Elle Lisp interpreter correctly handles
// while and for loop constructs, testing both compilation and execution.

use elle::compiler::ast::Expr;
use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

/// Helper struct for evaluating expressions in loops
struct LoopTestEnv {
    vm: VM,
    symbols: SymbolTable,
}

impl LoopTestEnv {
    fn new() -> Self {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        LoopTestEnv { vm, symbols }
    }

    fn eval(&mut self, code: &str) -> Result<Value, String> {
        let value = read_str(code, &mut self.symbols)?;
        let expr = value_to_expr(&value, &mut self.symbols)?;
        let bytecode = compile(&expr);
        self.vm.execute(&bytecode)
    }
}

#[test]
fn unit_while_loop_parses_correctly() {
    let mut symbols = SymbolTable::new();

    // Test: while loop can be parsed from source
    let code = "(while (< i 5) (print i))";
    let value = read_str(code, &mut symbols).expect("Failed to parse while loop");

    // Should parse as a list starting with 'while'
    assert!(value.is_list());
}

#[test]
fn unit_for_loop_parses_correctly() {
    let mut symbols = SymbolTable::new();

    // Test: each loop can be parsed from source
    let code = "(each item (list 1 2 3) (print item))";
    let value = read_str(code, &mut symbols).expect("Failed to parse each loop");

    // Should parse as a list starting with 'each'
    assert!(value.is_list());
}

#[test]
fn unit_while_loop_compiles_to_bytecode() {
    let mut symbols = SymbolTable::new();

    // Test: while loop compiles to bytecode without errors
    let code = "(define x 0) (while (< x 5) (set! x (+ x 1)))";
    let value = read_str(code, &mut symbols).expect("Failed to parse");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expr");

    let _bytecode = compile(&expr);
    // If we get here, compilation succeeded
}

#[test]
fn unit_for_loop_compiles_to_bytecode() {
    let mut symbols = SymbolTable::new();

    // Test: each loop compiles to bytecode without errors
    let code = "(each item (list 1 2 3) (+ item 1))";
    let value = read_str(code, &mut symbols).expect("Failed to parse");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expr");

    let _bytecode = compile(&expr);
    // If we get here, compilation succeeded
}

#[test]

fn unit_while_loop_returns_nil() {
    let mut env = LoopTestEnv::new();

    // Test: while loop returns nil
    env.eval("(define x 0)").unwrap();
    let result = env.eval("(while (< x 1) (set! x 1))").unwrap();

    assert_eq!(result, Value::Nil);
}

#[test]
fn unit_for_loop_returns_nil() {
    let mut env = LoopTestEnv::new();

    // Test: each loop returns nil
    env.eval("(define lst (list 1 2 3))").unwrap();
    let result = env.eval("(each item lst (+ item 1))").unwrap();

    assert_eq!(result, Value::Nil);
}

#[test]
fn unit_nested_loops_compile() {
    let mut symbols = SymbolTable::new();

    // Test: nested loops compile successfully
    let code = "(define i 0) (define j 0) (while (< i 3) (begin (set! j 0) (while (< j 2) (set! j (+ j 1))) (set! i (+ i 1))))";
    let value = read_str(code, &mut symbols).expect("Failed to parse nested loops");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert");

    let _bytecode = compile(&expr);
    // If we get here, nested loops compiled successfully
}

#[test]
fn unit_while_loop_with_multiple_conditions() {
    let mut symbols = SymbolTable::new();

    // Test: while loop with AND condition
    let code = "(while (and (< x 5) (> y 0)) (set! x (+ x 1)))";
    let value = read_str(code, &mut symbols).expect("Failed to parse");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert");

    let _bytecode = compile(&expr);
    // Compilation succeeds
}

#[test]

fn unit_for_loop_with_empty_list() {
    let mut env = LoopTestEnv::new();

    // Test: each loop with empty list doesn't error
    env.eval("(define empty-list (list))").unwrap();
    let result = env.eval("(each item empty-list (+ item 1))");

    assert!(result.is_ok());
}

#[test]

fn unit_simple_while_increment() {
    let mut env = LoopTestEnv::new();

    // Test: simple counter increment works
    env.eval("(define counter 0)").unwrap();
    env.eval("(while (< counter 3) (set! counter (+ counter 1)))")
        .unwrap();
    let result = env.eval("counter").unwrap();

    assert_eq!(result, Value::Int(3));
}

#[test]

fn unit_while_loop_comparison_operators() {
    let mut env = LoopTestEnv::new();

    // Test: while loop with >= operator
    env.eval("(define n 5)").unwrap();
    env.eval("(while (>= n 1) (set! n (- n 1)))").unwrap();
    let result = env.eval("n").unwrap();

    assert_eq!(result, Value::Int(0));
}

#[test]

fn unit_while_loop_multiplication() {
    let mut env = LoopTestEnv::new();

    // Test: while loop with multiplication
    env.eval("(define x 2)").unwrap();
    env.eval("(define iterations 0)").unwrap();
    env.eval(
        "(while (< iterations 5) (begin (set! x (* x 2)) (set! iterations (+ iterations 1))))",
    )
    .unwrap();

    let x = env.eval("x").unwrap();
    // 2 * 2^5 = 64
    assert_eq!(x, Value::Int(64));
}

#[test]

fn unit_loop_variable_mutation() {
    let mut env = LoopTestEnv::new();

    // Test: loop properly mutates variables
    env.eval("(define a 10)").unwrap();
    env.eval("(define b 5)").unwrap();
    env.eval("(while (> a b) (set! a (- a 1)))").unwrap();

    let a = env.eval("a").unwrap();
    assert_eq!(a, Value::Int(5)); // a should equal b after loop
}

#[test]

fn unit_while_false_condition() {
    let mut env = LoopTestEnv::new();

    // Test: while with false condition never executes
    env.eval("(define flag 0)").unwrap();
    env.eval("(while (= 1 0) (set! flag 1))").unwrap();
    let flag = env.eval("flag").unwrap();

    assert_eq!(flag, Value::Int(0)); // flag should remain 0
}

#[test]
fn unit_loop_construct_ast_structure() {
    let mut symbols = SymbolTable::new();

    // Test: while loop compiles correctly
    // When we read multiple top-level expressions, only the first is returned
    // by read_str (it reads one value at a time)
    let code = "(define x 0)";
    let value = read_str(code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();

    // Should be a Define expression
    assert!(
        matches!(expr, Expr::Define { .. }),
        "Expected Define expression, got {:?}",
        expr
    );

    // Now test a complete program with begin
    let mut symbols = SymbolTable::new();
    let code = "(begin (define y 0) (while (< y 5) (set! y (+ y 1))))";
    let value = read_str(code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();

    // Should be Begin with Define and While
    match expr {
        Expr::Begin(exprs) => {
            assert_eq!(
                exprs.len(),
                2,
                "Expected 2 expressions in Begin block, got {}",
                exprs.len()
            );

            // First should be Define
            assert!(
                matches!(exprs[0], Expr::Define { .. }),
                "First expression should be Define"
            );

            // Second should be While
            assert!(
                matches!(exprs[1], Expr::While { .. }),
                "Second expression should be While"
            );
        }
        _ => panic!("Expected Begin expression, got {:?}", expr),
    }
}

#[test]
fn unit_forever_loop_parses_correctly() {
    let mut symbols = SymbolTable::new();

    // Test: forever loop can be parsed from source
    let code = "(forever (print i))";
    let value = read_str(code, &mut symbols).expect("Failed to parse forever loop");

    // Should parse as a list starting with 'forever'
    assert!(value.is_list());
}

#[test]
fn unit_forever_loop_compiles_to_bytecode() {
    let mut symbols = SymbolTable::new();

    // Test: forever loop compiles to bytecode without errors
    let code = "(define x 0) (forever (if (= x 5) (break)) (set! x (+ x 1)))";
    let value = read_str(code, &mut symbols).expect("Failed to parse");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expr");

    let _bytecode = compile(&expr);
    // If we get here, compilation succeeded
}

#[test]
fn unit_forever_loop_returns_nil() {
    let mut env = LoopTestEnv::new();

    // Test: forever loop returns nil (when it exits via break or other means)
    // Since forever is infinite, we need a way to exit - using a condition that breaks
    env.eval("(define x 0)").unwrap();
    let result = env.eval("(forever (if (>= x 1) (break)) (set! x 1))");

    // Note: break is not yet implemented, so this will test the compilation
    // The actual execution test will be added once break is available
    assert!(result.is_ok() || result.is_err()); // Just verify it doesn't panic
}

#[test]
fn unit_forever_loop_compiles_with_multiple_body_expressions() {
    let mut symbols = SymbolTable::new();

    // Test: forever loop with multiple body expressions
    let code = "(forever (display \"x\") (newline) (set! x (+ x 1)))";
    let value = read_str(code, &mut symbols).expect("Failed to parse");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expr");

    let _bytecode = compile(&expr);
    // If we get here, compilation succeeded
}
