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

// === Each-loop variable scoping ===

#[test]
fn test_for_loop_variable_not_in_global_scope() {
    // After an each loop, the loop variable should not be accessible
    let mut eval = ScopeEval::new();
    eval.eval("(each x (list 1 2 3) (+ x 1))").unwrap();
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
    eval.eval("(each x (list 1 2 3) (set! result (+ result x)))")
        .unwrap();
    assert_eq!(eval.eval("result").unwrap(), Value::Int(6));
}

#[test]
fn test_for_loop_does_not_clobber_outer_variable() {
    let mut eval = ScopeEval::new();
    eval.eval("(define x 999)").unwrap();
    eval.eval("(each x (list 1 2 3) (+ x 1))").unwrap();
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
    eval.eval("(each x (list 1 2 3) (define y (* x 10)))")
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
    eval.eval("(each i (list 1 2 3) (set! result (+ result i)))")
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

// === letrec: mutual recursion ===

#[test]
fn test_letrec_mutual_recursion_even_odd() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (letrec ((is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
                 (is-odd  (lambda (n) (if (= n 0) #f (is-even (- n 1))))))
          (is-even 10))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_letrec_mutual_recursion_odd() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (letrec ((is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
                 (is-odd  (lambda (n) (if (= n 0) #f (is-even (- n 1))))))
          (is-odd 7))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_letrec_self_recursion() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (letrec ((fact (lambda (n) (if (= n 0) 1 (* n (fact (- n 1)))))))
          (fact 5))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(120));
}

#[test]
fn test_letrec_three_way_cycle() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (letrec ((a (lambda (n) (if (= n 0) "a" (b (- n 1)))))
                 (b (lambda (n) (if (= n 0) "b" (c (- n 1)))))
                 (c (lambda (n) (if (= n 0) "c" (a (- n 1))))))
          (a 5))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::String(std::rc::Rc::from("c")));
}

#[test]
fn test_letrec_with_non_lambda_bindings() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (letrec ((x 10)
                 (y 20))
          (+ x y))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(30));
}

#[test]
fn test_letrec_nested_in_lambda() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        ((lambda (base)
           (letrec ((is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
                    (is-odd  (lambda (n) (if (= n 0) #f (is-even (- n 1))))))
             (is-even base)))
         6)
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Bool(true));
}

// === define inside lambda bodies ===

#[test]
fn test_define_in_lambda_does_not_leak() {
    let mut eval = ScopeEval::new();
    eval.eval(
        r#"
        (define run (lambda ()
          (begin
            (define local-var 42)
            local-var)))
    "#,
    )
    .unwrap();
    eval.eval("(run)").unwrap();
    let result = eval.eval("local-var");
    assert!(
        result.is_err(),
        "define inside lambda body should not leak to globals"
    );
}

#[test]
fn test_define_in_lambda_self_recursion() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (begin
          (define run (lambda ()
            (begin
              (define fact (lambda (n) (if (= n 0) 1 (* n (fact (- n 1))))))
              (fact 6))))
          (run))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(720));
}

#[test]
fn test_define_in_lambda_mutual_recursion() {
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (begin
          (define run (lambda ()
            (begin
              (define is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
              (define is-odd (lambda (n) (if (= n 0) #f (is-even (- n 1)))))
              (is-even 8))))
          (run))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Bool(true));
}

// === tail call optimization ===

#[test]
fn test_tail_call_simple() {
    // Test that tail calls work for simple recursion
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (begin
          (define countdown (lambda (n) (if (= n 0) 0 (countdown (- n 1)))))
          (countdown 100))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(0));
}

#[test]
fn test_tail_call_accumulator() {
    // Test tail call with accumulator pattern
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (begin
          (define sum-to (lambda (n acc)
            (if (= n 0) acc (sum-to (- n 1) (+ acc n)))))
          (sum-to 100 0))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(5050));
}

#[test]
fn test_tail_call_mutual() {
    // Test mutual tail recursion
    let mut eval = ScopeEval::new();
    let result = eval
        .eval(
            r#"
        (begin
          (define count-down-even (lambda (n)
            (if (= n 0) 0 (count-down-odd (- n 1)))))
          (define count-down-odd (lambda (n)
            (if (= n 0) 1 (count-down-even (- n 1)))))
          (count-down-even 100))
    "#,
        )
        .unwrap();
    assert_eq!(result, Value::Int(0));
}
