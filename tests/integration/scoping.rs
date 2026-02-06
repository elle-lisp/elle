use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

struct ScopeEval {
    vm: VM,
    symbols: SymbolTable,
}

impl ScopeEval {
    fn new() -> Self {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        ScopeEval { vm, symbols }
    }

    fn eval(&mut self, code: &str) -> Result<Value, String> {
        let value = read_str(code, &mut self.symbols)?;
        let expr = value_to_expr(&value, &mut self.symbols)?;
        let bytecode = compile(&expr);
        self.vm.execute(&bytecode)
    }
}

// === For-loop variable scoping ===

#[test]
fn test_for_loop_variable_not_in_global_scope() {
    // After a for loop, the loop variable should not be accessible
    let mut eval = ScopeEval::new();
    eval.eval("(for x (list 1 2 3) (+ x 1))").unwrap();
    let result = eval.eval("x");
    assert!(
        result.is_err(),
        "Loop variable 'x' should not leak to global scope"
    );
}

#[test]
fn test_for_loop_variable_accessible_in_body() {
    let mut eval = ScopeEval::new();
    eval.eval("(define result 0)").unwrap();
    eval.eval("(for x (list 1 2 3) (set! result (+ result x)))")
        .unwrap();
    assert_eq!(eval.eval("result").unwrap(), Value::Int(6));
}

#[test]
fn test_for_loop_does_not_clobber_outer_variable() {
    let mut eval = ScopeEval::new();
    eval.eval("(define x 999)").unwrap();
    eval.eval("(for x (list 1 2 3) (+ x 1))").unwrap();
    assert_eq!(eval.eval("x").unwrap(), Value::Int(999));
}

// === Define inside loops ===

#[test]
fn test_define_in_while_loop_is_local() {
    let mut eval = ScopeEval::new();
    eval.eval("(define i 0)").unwrap();
    eval.eval("(while (< i 3) (begin (define temp (* i 2)) (set! i (+ i 1))))")
        .unwrap();
    let result = eval.eval("temp");
    assert!(
        result.is_err(),
        "Variable defined inside while loop should not leak"
    );
}

#[test]
fn test_define_in_for_loop_is_local() {
    let mut eval = ScopeEval::new();
    eval.eval("(for x (list 1 2 3) (define y (* x 10)))")
        .unwrap();
    let result = eval.eval("y");
    assert!(
        result.is_err(),
        "Variable defined inside for loop should not leak"
    );
}

// === Block form ===

#[test]
fn test_block_creates_scope() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(block (define x 42) x)").unwrap();
    assert_eq!(result, Value::Int(42));
    let outer = eval.eval("x");
    assert!(outer.is_err(), "Variable defined in block should not leak");
}

#[test]
fn test_block_returns_last_expression() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(block 1 2 3)").unwrap();
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_nested_blocks() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval("(block (define x 1) (block (define y 2) (+ x y)))")
        .unwrap();
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_block_inside_lambda() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval("((lambda (x) (block (define y (* x 2)) (+ x y))) 5)")
        .unwrap();
    assert_eq!(result, Value::Int(15));
}

// === Variable shadowing ===

#[test]
fn test_set_bang_modifies_innermost_scope() {
    let mut eval = ScopeEval::new();
    eval.eval("(define x 1)").unwrap();
    eval.eval("(block (define x 10) (set! x 20))").unwrap();
    // The global x should be unchanged
    assert_eq!(eval.eval("x").unwrap(), Value::Int(1));
}

#[test]
fn test_let_shadowing() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(let ((x 1)) (let ((x 2)) x))").unwrap();
    assert_eq!(result, Value::Int(2));
}

#[test]
fn test_for_loop_variable_shadows_global() {
    let mut eval = ScopeEval::new();
    eval.eval("(define x 100)").unwrap();
    eval.eval("(define sum 0)").unwrap();
    eval.eval("(for x (list 1 2 3) (set! sum (+ sum x)))")
        .unwrap();
    assert_eq!(eval.eval("sum").unwrap(), Value::Int(6));
    assert_eq!(eval.eval("x").unwrap(), Value::Int(100)); // x unchanged
}

// === Gensym ===

#[test]
fn test_gensym_returns_string() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(string? (gensym))").unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_gensym_unique() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(= (gensym) (gensym))").unwrap();
    assert_eq!(result, Value::Bool(false));
}

// === Existing behavior preserved ===

#[test]
fn test_global_define_still_works() {
    let mut eval = ScopeEval::new();
    eval.eval("(define x 42)").unwrap();
    assert_eq!(eval.eval("x").unwrap(), Value::Int(42));
}

#[test]
fn test_let_still_works() {
    let mut eval = ScopeEval::new();
    let result = eval.eval("(let ((x 5) (y 10)) (+ x y))").unwrap();
    assert_eq!(result, Value::Int(15));
}

#[test]
fn test_while_loop_still_works() {
    let mut eval = ScopeEval::new();
    eval.eval("(define counter 0)").unwrap();
    eval.eval("(while (< counter 5) (set! counter (+ counter 1)))")
        .unwrap();
    assert_eq!(eval.eval("counter").unwrap(), Value::Int(5));
}

#[test]
fn test_for_loop_accumulation_still_works() {
    let mut eval = ScopeEval::new();
    eval.eval("(define result 0)").unwrap();
    eval.eval("(for i (list 1 2 3) (set! result (+ result i)))")
        .unwrap();
    assert_eq!(eval.eval("result").unwrap(), Value::Int(6));
}

#[test]
fn test_closure_capture_still_works() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval("(begin (define make-adder (lambda (x) (lambda (y) (+ x y)))) (define add5 (make-adder 5)) (add5 10))")
        .unwrap();
    assert_eq!(result, Value::Int(15));
}

#[test]
fn test_gcd_with_define_in_loop() {
    // This is the existing GCD test pattern — should still work
    let mut eval = ScopeEval::new();
    eval.eval("(define a 48)").unwrap();
    eval.eval("(define b 18)").unwrap();
    eval.eval("(while (> b 0) (begin (define temp (% a b)) (set! a b) (set! b temp)))")
        .unwrap();
    assert_eq!(eval.eval("a").unwrap(), Value::Int(6));
}
