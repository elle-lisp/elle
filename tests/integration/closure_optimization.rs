// DEFENSE: Integration tests for closure optimization (Issue #20)
// These tests verify closure correctness and provide baseline for optimization work

use elle::{compile, list, register_primitives, Lexer, Reader, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::new(tokens);
    let mut values = Vec::new();
    while let Some(result) = reader.try_read(&mut symbols) {
        values.push(result?);
    }

    let value = if values.len() == 1 {
        values.into_iter().next().unwrap()
    } else if values.is_empty() {
        return Err("No input".to_string());
    } else {
        let mut begin_args = vec![Value::Symbol(symbols.intern("begin"))];
        begin_args.extend(values);
        list(begin_args)
    };

    let expr = elle::compiler::converters::value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============================================================================
// Closure Optimization Tests (Issue #20) - Baseline for Capture Analysis
// These tests work with single-level closures that capture globals
// ============================================================================

#[test]
fn test_closure_captures_global_variable() {
    let code = r#"
        (define x 42)
        ((lambda () x))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_closure_captures_function() {
    let code = r#"
        (define add (lambda (a b) (+ a b)))
        ((lambda () (add 10 20)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_no_captures() {
    let code = r#"
        ((lambda () 42))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_closure_captures_multiple_globals() {
    let code = r#"
        (define x 10)
        (define y 20)
        ((lambda () (+ x y)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_references_global_list() {
    let code = r#"
        (define nums (list 1 2 3))
        ((lambda () (first nums)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(1));
}

#[test]
fn test_closure_captures_and_uses_global() {
    let code = r#"
        (define threshold 5)
        ((lambda (x) (> x threshold)) 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Bool(true));
}
#[test]
fn test_closure_shadowing_parameter() {
    let code = r#"
        (define x 100)
        ((lambda (x) x) 42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_closure_with_multiple_operations() {
    let code = r#"
        (define base 10)
        ((lambda (x y)
          (+ (* base x) y))
         2 3)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(23));
}

#[test]
fn test_closure_with_conditional_using_capture() {
    let code = r#"
        (define min-val 0)
        ((lambda (x)
          (if (> x min-val) x min-val))
         5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5));
}
