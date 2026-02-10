// Unit tests for scope management in the compiler (Phase 1)
//
// These tests verify that the compiler correctly tracks variable scopes
// and generates proper AST with scope depth and index information.
// This is Phase 1 of the scope implementation.

use elle::compiler::ast::Expr;
use elle::compiler::converters::value_to_expr;
use elle::compiler::scope::ScopeType;
use elle::{read_str, SymbolTable, Value};

/// Test basic scope structure creation
#[test]
fn test_scope_type_enum_exists() {
    // Verify that ScopeType enum is available
    let _global = ScopeType::Global;
    let _function = ScopeType::Function;
    let _block = ScopeType::Block;
    let _loop = ScopeType::Loop;
    let _let = ScopeType::Let;
}

/// Test that scope types can be created
#[test]
fn test_scope_types_can_be_created() {
    // Test that ScopeType variants can be created and used
    let _block = ScopeType::Block;
    let _loop_scope = ScopeType::Loop;
    let _let_scope = ScopeType::Let;

    // Verify they're valid
    assert_eq!(ScopeType::Block, ScopeType::Block);
}

/// Test that variables in lambdas have correct depth
#[test]
fn test_lambda_variable_depth() {
    let mut symbols = SymbolTable::new();

    // Test: (lambda (x) x) - parameter should be accessible
    let code = "(lambda (x) x)";
    let value = read_str(code, &mut symbols).expect("Failed to parse lambda");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Lambda expression
    match expr {
        Expr::Lambda { params, body, .. } => {
            assert_eq!(params.len(), 1);
            // Body should be a reference to parameter x at depth 0
            match *body {
                Expr::Var(_, depth, _) => {
                    assert_eq!(depth, 0); // Parameter is at depth 0
                }
                _ => panic!("Expected Var expression in lambda body"),
            }
        }
        _ => panic!("Expected Lambda expression"),
    }
}

/// Test nested lambda variable scoping
#[test]
fn test_nested_lambda_variable_scope() {
    let mut symbols = SymbolTable::new();

    // Test: (lambda (x) (lambda (y) x)) - captured variable should have depth 1
    let code = "(lambda (x) (lambda (y) x))";
    let value = read_str(code, &mut symbols).expect("Failed to parse nested lambda");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Outer lambda
    match expr {
        Expr::Lambda { params, body, .. } => {
            assert_eq!(params.len(), 1); // (x)
                                         // Body should be inner lambda
            match *body {
                Expr::Lambda {
                    params: inner_params,
                    body: inner_body,
                    captures,
                    locals: _,
                } => {
                    assert_eq!(inner_params.len(), 1); // (y)
                                                       // Inner body references outer x (should be in captures)
                    assert_eq!(captures.len(), 1);
                    // Body is the reference to captured x
                    match *inner_body {
                        Expr::Var(_, depth, _) => {
                            assert_eq!(depth, 1); // Outer parameter is at depth 1
                        }
                        _ => panic!("Expected Var in inner lambda body"),
                    }
                }
                _ => panic!("Expected Lambda as outer body"),
            }
        }
        _ => panic!("Expected Lambda expression"),
    }
}

/// Test that global variables are marked as GlobalVar
#[test]
fn test_global_variable_reference() {
    let mut symbols = SymbolTable::new();

    // Test: + should be a global variable
    let code = "+";
    let value = read_str(code, &mut symbols).expect("Failed to parse symbol");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a GlobalVar since + is not bound locally
    match expr {
        Expr::GlobalVar(_) => {
            // Correct
        }
        _ => panic!("Expected GlobalVar for undefined symbol"),
    }
}

/// Test let binding scoping
#[test]
fn test_let_binding_scope() {
    let mut symbols = SymbolTable::new();

    // Test: (let ((x 5)) (+ x 1)) - x should be accessible in body
    let code = "(let ((x 5)) (+ x 1))";
    let value = read_str(code, &mut symbols).expect("Failed to parse let");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // let gets converted to a lambda call
    match expr {
        Expr::Call { func, args, .. } => {
            // func should be a lambda
            match *func {
                Expr::Lambda { params, .. } => {
                    // Parameters are the let-bound variables
                    assert_eq!(params.len(), 1); // x
                }
                _ => panic!("Expected Lambda as let conversion"),
            }
            // args should be the binding expressions
            assert_eq!(args.len(), 1); // (5)
        }
        _ => panic!("Expected Call expression from let conversion"),
    }
}

/// Test that loop variables are handled
#[test]
fn test_while_loop_parsing() {
    let mut symbols = SymbolTable::new();

    // Test: (while condition body) - should parse correctly
    let code = "(while (< i 5) (set! i (+ i 1)))";
    let value = read_str(code, &mut symbols).expect("Failed to parse while loop");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a While expression
    match expr {
        Expr::While { cond, body } => {
            // Condition should be (< i 5)
            match *cond {
                Expr::Call { .. } => {
                    // Correct
                }
                _ => panic!("Expected Call in condition"),
            }
            // Body should be (set! i (+ i 1))
            match *body {
                Expr::Set { .. } => {
                    // Correct
                }
                _ => panic!("Expected Set in body"),
            }
        }
        _ => panic!("Expected While expression"),
    }
}

/// Test for loop parsing
#[test]
fn test_for_loop_parsing() {
    let mut symbols = SymbolTable::new();

    // Test: (for item items body) - should parse correctly
    let code = "(for item (list 1 2 3) (print item))";
    let value = read_str(code, &mut symbols).expect("Failed to parse for loop");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a For expression
    match expr {
        Expr::For { var, iter, body } => {
            // var should be the symbol 'item'
            let item_sym = symbols.intern("item");
            assert_eq!(var, item_sym);
            // iter should be (list 1 2 3)
            match *iter {
                Expr::Call { .. } => {
                    // Correct
                }
                _ => panic!("Expected Call in iterator"),
            }
            // body should be (print item)
            match *body {
                Expr::Call { .. } => {
                    // Correct
                }
                _ => panic!("Expected Call in body"),
            }
        }
        _ => panic!("Expected For expression"),
    }
}

/// Test let* sequential binding
#[test]
fn test_let_star_sequential_binding() {
    let mut symbols = SymbolTable::new();

    // Test: (let* ((x 1) (y (+ x 1))) (+ x y))
    // In let*, y can reference x because it's evaluated sequentially
    let code = "(let* ((x 1) (y (+ x 1))) (+ x y))";
    let value = read_str(code, &mut symbols).expect("Failed to parse let*");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Call (lambda transformation)
    match expr {
        Expr::Call { func, args, .. } => {
            // func should be lambda with two parameters
            match *func {
                Expr::Lambda { params, .. } => {
                    assert_eq!(params.len(), 2); // x and y
                }
                _ => panic!("Expected Lambda"),
            }
            // args should be two expressions: 1 and (+ x 1)
            assert_eq!(args.len(), 2);
        }
        _ => panic!("Expected Call expression"),
    }
}

/// Test that set! tracks depth correctly
#[test]
fn test_set_bang_depth_tracking() {
    let mut symbols = SymbolTable::new();

    // Test: (set! x 5) at global scope - depth should be MAX for globals
    let code = "(set! x 5)";
    let value = read_str(code, &mut symbols).expect("Failed to parse set!");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Set expression
    match expr {
        Expr::Set {
            var: _,
            depth,
            index,
            value: _,
        } => {
            // Global set should have index = MAX
            assert_eq!(index, usize::MAX);
            assert_eq!(depth, 0); // No local scopes
        }
        _ => panic!("Expected Set expression"),
    }
}

/// Test variable references in function calls
#[test]
fn test_variable_in_function_call() {
    let mut symbols = SymbolTable::new();

    // Test: (define x 5) followed by (+ x 1)
    let code = "(+ x 1)";
    let value = read_str(code, &mut symbols).expect("Failed to parse call");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Call with arguments
    match expr {
        Expr::Call { args, .. } => {
            assert_eq!(args.len(), 2);
            // First argument is x (undefined, so global)
            match &args[0] {
                Expr::GlobalVar(_) => {
                    // Correct - x is not locally bound
                }
                _ => panic!("Expected GlobalVar for undefined x"),
            }
            // Second argument is 1 (literal)
            match &args[1] {
                Expr::Literal(Value::Int(1)) => {
                    // Correct
                }
                _ => panic!("Expected Literal(1)"),
            }
        }
        _ => panic!("Expected Call expression"),
    }
}

/// Test function parameter binding
#[test]
fn test_function_parameter_binding() {
    let mut symbols = SymbolTable::new();

    // Test: ((lambda (x y) (+ x y)) 1 2)
    let code = "((lambda (x y) (+ x y)) 1 2)";
    let value = read_str(code, &mut symbols).expect("Failed to parse function call");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Call where func is a Lambda
    match expr {
        Expr::Call { func, args, .. } => {
            assert_eq!(args.len(), 2); // 1 and 2
            match *func {
                Expr::Lambda { params, body, .. } => {
                    assert_eq!(params.len(), 2); // x and y
                                                 // Body should reference x and y with depth 0
                    match *body {
                        Expr::Call {
                            func: _,
                            args: body_args,
                            ..
                        } => {
                            assert_eq!(body_args.len(), 2);
                            // Both args should reference parameters (depth 0)
                            match &body_args[0] {
                                Expr::Var(_, depth, _) => {
                                    assert_eq!(*depth, 0);
                                }
                                _ => panic!("Expected Var for x"),
                            }
                            match &body_args[1] {
                                Expr::Var(_, depth, _) => {
                                    assert_eq!(*depth, 0);
                                }
                                _ => panic!("Expected Var for y"),
                            }
                        }
                        _ => panic!("Expected Call in lambda body"),
                    }
                }
                _ => panic!("Expected Lambda as function"),
            }
        }
        _ => panic!("Expected Call expression"),
    }
}

/// Test that literal values have no scope issues
#[test]
fn test_literal_values() {
    let mut symbols = SymbolTable::new();

    // Test: 42 should just be a literal
    let code = "42";
    let value = read_str(code, &mut symbols).expect("Failed to parse literal");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    match expr {
        Expr::Literal(Value::Int(42)) => {
            // Correct
        }
        _ => panic!("Expected Literal expression"),
    }
}

/// Test quoted forms have no scope issues
#[test]
fn test_quoted_form() {
    let mut symbols = SymbolTable::new();

    // Test: '(+ 1 2) should be quoted
    let code = "'(+ 1 2)";
    let value = read_str(code, &mut symbols).expect("Failed to parse quote");
    let expr = value_to_expr(&value, &mut symbols).expect("Failed to convert to expression");

    // Should be a Literal (the quoted form)
    match expr {
        Expr::Literal(_) => {
            // Correct
        }
        _ => panic!("Expected Literal for quoted form"),
    }
}
