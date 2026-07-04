use super::*;

// === Control Flow: cond ===

#[test]
fn test_eval_cond_first_true() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(cond true 42)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_second_true() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(cond false 1 true 42)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_else() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(cond false 1 false 2 42)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_with_expressions() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(cond (< 5 10) (+ 20 22))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

// === Control Flow: and ===

#[test]
fn test_eval_and_all_true() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(and true true true)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_eval_and_one_false() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(and true false true)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_and_returns_last() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(and 1 2 3)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_and_short_circuit() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    // If and doesn't short-circuit, this would fail trying to call nil
    let result = eval("(and false (nil))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_and_empty() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(and)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

// === Control Flow: or ===

#[test]
fn test_eval_or_all_false() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(or false false false)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_or_one_true() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(or false true false)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_eval_or_returns_first_truthy() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(or false 42 99)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_destructure_list_basic() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    // In the file-as-letrec model, bindings are local to each compilation
    // unit. Use a single expression to test destructuring.
    let result = eval(
        "(begin (def (a b c) (list 1 2 3)) (list a b c))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    let v = result.unwrap();
    assert_eq!(v.as_pair().unwrap().first.as_int(), Some(1));
    let rest1 = v.as_pair().unwrap().rest;
    assert_eq!(rest1.as_pair().unwrap().first.as_int(), Some(2));
    let rest2 = rest1.as_pair().unwrap().rest;
    assert_eq!(rest2.as_pair().unwrap().first.as_int(), Some(3));
}

#[test]
fn test_eval_or_empty() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(or)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

// === Control Flow: while ===

#[test]
fn test_eval_while_never_executes() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(while false 42)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_eval_while_with_mutation() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(begin (var x 0) (while (< x 5) (assign x (+ x 1))) x)",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(5));
}

// === Closures and Captures ===

#[test]
fn test_eval_closure_captures_local() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(let [x 10] ((fn () x)))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_eval_closure_captures_multiple() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(let [x 10 y 20] ((fn () (+ x y))))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_eval_nested_closure() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(let [x 10] ((fn () ((fn () x)))))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_eval_closure_with_param_and_capture() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(let [x 10] ((fn (y) (+ x y)) 5))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(15));
}

// === Higher-Order Functions ===

#[test]
fn test_eval_function_as_argument() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "((fn (f x) (f x)) (fn (n) (+ n 1)) 10)",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(11));
}

#[test]
fn test_eval_function_returning_function() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(((fn (x) (fn (y) (+ x y))) 10) 5)",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(15));
}

// === Define and Assign ===

#[test]
fn test_eval_define_then_use() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(begin (var x 42) x)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_define_then_set() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(begin (var x 10) (assign x 42) x)",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_set_in_closure() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(begin
           (var counter 0)
           (def inc (fn () (assign counter (+ counter 1))))
           (inc)
           (inc)
           counter)",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_intrinsic_fib() {
    // Fibonacci exercises intrinsic specialization with double recursion
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(begin
            (def fib (fn (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))
            (fib 10))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(55));
}

#[test]
fn test_intrinsic_unary_neg() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(- 5)", symbols, vm, cctx, "<test>").unwrap(),
        Value::int(-5)
    );
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(- -3)", symbols, vm, cctx, "<test>").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_intrinsic_variadic_fallthrough() {
    // Variadic + falls through to generic call
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(+ 1 2 3)", symbols, vm, cctx, "<test>").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_intrinsic_not() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(not true)", symbols, vm, cctx, "<test>").unwrap(),
        Value::bool(false)
    );
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(not false)", symbols, vm, cctx, "<test>").unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_intrinsic_rem() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    assert_eq!(
        eval("(rem 17 5)", symbols, vm, cctx, "<test>").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_fold_multiple_elements() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();

    // Test with (list 1) - should work
    let code1 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1)))"#;

    let result1 = eval(code1, symbols, vm, cctx, "<test>");
    println!("list 1: {:?}", result1);

    // Test with (list 1 2) - might fail
    let mut rt2 = setup();
    let (vm2, symbols2, cctx2) = rt2.parts();
    let code2 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2)))"#;

    let result2 = eval(code2, symbols2, vm2, cctx2, "<test>");
    println!("list 1 2: {:?}", result2);

    // Test with (list 1 2 3) - original failing case
    let mut rt3 = setup();
    let (vm3, symbols3, cctx3) = rt3.parts();
    let code3 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2 3)))"#;

    let result3 = eval(code3, symbols3, vm3, cctx3, "<test>");
    println!("list 1 2 3: {:?}", result3);
}
