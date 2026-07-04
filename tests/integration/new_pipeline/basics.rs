use super::*;

// ============ Literal Tests ============

#[test]
fn test_literal_int() {
    assert!(compiles("42"));
}

#[test]
fn test_literal_float() {
    assert!(compiles("3.14"));
}

#[test]
fn test_literal_bool_true() {
    assert!(compiles("true"));
}

#[test]
fn test_literal_bool_false() {
    assert!(compiles("false"));
}

#[test]
fn test_literal_nil() {
    assert!(compiles("nil"));
}

#[test]
fn test_literal_string() {
    assert!(compiles("\"hello world\""));
}

#[test]
fn test_literal_keyword() {
    assert!(compiles(":foo"));
}

// ============ Control Flow Tests ============

#[test]
fn test_if_simple() {
    assert!(compiles("(if true 1 2)"));
}

#[test]
fn test_if_nested() {
    assert!(compiles("(if (if true true false) 1 2)"));
}

#[test]
fn test_cond_simple() {
    assert!(compiles("(cond true 1)"));
}

#[test]
fn test_cond_with_else() {
    assert!(compiles("(cond false 1 2)"));
}

#[test]
fn test_cond_multiple_clauses() {
    assert!(compiles("(cond false 1 false 2 true 3 4)"));
}

#[test]
fn test_and_empty() {
    assert!(compiles("(and)"));
}

#[test]
fn test_and_single() {
    assert!(compiles("(and true)"));
}

#[test]
fn test_and_multiple() {
    assert!(compiles("(and true true false)"));
}

#[test]
fn test_or_empty() {
    assert!(compiles("(or)"));
}

#[test]
fn test_or_single() {
    assert!(compiles("(or false)"));
}

#[test]
fn test_or_multiple() {
    assert!(compiles("(or false false true)"));
}

// ============ Binding Tests ============

#[test]
fn test_let_simple() {
    assert!(compiles("(let [x 10] x)"));
}

#[test]
fn test_let_multiple_bindings() {
    assert!(compiles("(let [x 1 y 2] x)"));
}

#[test]
fn test_let_nested() {
    assert!(compiles("(let [x 1] (let [y 2] x))"));
}

#[test]
fn test_letrec_simple() {
    assert!(compiles("(letrec [x 10] x)"));
}

#[test]
fn test_define() {
    assert!(compiles("(var x 42)"));
}

// ============ Function Tests ============

#[test]
fn test_lambda_identity() {
    assert!(compiles("(fn (x) x)"));
}

#[test]
fn test_lambda_const() {
    assert!(compiles("(fn () 42)"));
}

#[test]
fn test_lambda_multiple_params() {
    assert!(compiles("(fn (x y z) x)"));
}

#[test]
fn test_lambda_with_body() {
    assert!(compiles("(fn (x) (begin x x))"));
}

#[test]
fn test_call_simple() {
    let mut symbols = SymbolTable::new();
    let result = compile("(%add 1 2)", &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
}

#[test]
fn test_call_nested() {
    let mut symbols = SymbolTable::new();
    let result = compile("(%add (%add 1 2) 3)", &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
}

// ============ Loop Tests ============

#[test]
fn test_while_simple() {
    assert!(compiles("(while false nil)"));
}

#[test]
fn test_each_simple() {
    eval_source(
        "(let [@sum 0] (each x '(1 2 3) (assign sum (+ sum x))) sum)",
        |result| assert_eq!(result.unwrap().as_int().unwrap(), 6),
    );
}

#[test]
fn test_each_with_in() {
    eval_source(
        "(let [@sum 0] (each x in '(1 2 3) (assign sum (+ sum x))) sum)",
        |result| assert_eq!(result.unwrap().as_int().unwrap(), 6),
    );
}

// ============ Sequence Tests ============

#[test]
fn test_begin_empty() {
    assert!(compiles("(begin)"));
}

#[test]
fn test_begin_single() {
    assert!(compiles("(begin 42)"));
}

#[test]
fn test_begin_multiple() {
    assert!(compiles("(begin 1 2 3)"));
}

#[test]
fn test_block() {
    assert!(compiles("(block 1 2 3)"));
}

// ============ Quote Tests ============

#[test]
fn test_quote_symbol() {
    assert!(compiles("'foo"));
}

#[test]
fn test_quote_list() {
    assert!(compiles("'(1 2 3)"));
}

// ============ Exception Tests ============

#[test]
fn test_try_simple() {
    assert!(compiles("(try 42 (catch e e))"));
}

// ============ Yield Tests ============

#[test]
fn test_yield() {
    assert!(compiles("(yield 42)"));
}
