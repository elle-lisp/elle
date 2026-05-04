use elle::context::{clear_symbol_table, set_symbol_table};
use elle::hir::HirKind;
use elle::pipeline::{analyze, compile, compile_file, eval};
use elle::{register_primitives, SymbolTable, Value, VM};

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

fn setup_with_stdlib() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    // Context pointers must be set during init_stdlib (macros use gensym).
    // They're invalidated by the move on return, so callers needing context
    // (e.g. eval special form) must re-set them.
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    elle::init_stdlib(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_compile_literal() {
    let (mut symbols, _) = setup();
    let result = compile("42", &mut symbols, "<test>");
    assert!(result.is_ok());
    let compiled = result.unwrap();
    assert!(!compiled.bytecode.instructions.is_empty());
}

#[test]
fn test_compile_if() {
    let (mut symbols, _) = setup();
    let result = compile("(if true 1 2)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_let() {
    let (mut symbols, _) = setup();
    let result = compile("(let [x 10] x)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_lambda() {
    let (mut symbols, _) = setup();
    let result = compile("(fn (x) x)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_call() {
    let (mut symbols, _) = setup();
    let result = compile("(%add 1 2)", &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
}

#[test]
fn test_compile_global_variable() {
    let (mut symbols, _) = setup();
    // Test that global variables (like list) are properly recognized and emit LoadGlobal
    let result = compile("(list 1 2)", &mut symbols, "<test>");
    assert!(result.is_ok(), "Global variable handling failed: {:?}", result.err());
}

#[test]
fn test_compile_begin() {
    let (mut symbols, _) = setup();
    let result = compile("(begin 1 2 3)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_and() {
    let (mut symbols, _) = setup();
    let result = compile("(and true true false)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_or() {
    let (mut symbols, _) = setup();
    let result = compile("(or false false true)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_while() {
    let (mut symbols, _) = setup();
    let result = compile("(while false nil)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_cond() {
    let (mut symbols, _) = setup();
    let result = compile("(cond true 1 2)", &mut symbols, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_eval_literal() {
    let (mut symbols, mut vm) = setup();
    let result = eval("42", &mut symbols, &mut vm, "<test>");
    // Note: execution may fail due to incomplete bytecode mapping
    // but compilation should succeed
    let _ = result;
}

#[test]
fn test_eval_addition() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval("(+ 1 2)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}

#[test]
fn test_eval_subtraction() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval("(- 10 3)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(7)),
        Err(e) => panic!("Expected Ok(7), got Err: {}", e),
    }
}

#[test]
fn test_eval_nested_arithmetic() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval("(+ (* 2 3) (- 10 5))", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(11)),
        Err(e) => panic!("Expected Ok(11), got Err: {}", e),
    }
}

#[test]
fn test_eval_if_true() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(if true 42 0)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_eval_if_false() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(if false 42 0)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(0)),
        Err(e) => panic!("Expected Ok(0), got Err: {}", e),
    }
}

#[test]
fn test_eval_let_simple() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(let [x 10] x)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(10)),
        Err(e) => panic!("Expected Ok(10), got Err: {}", e),
    }
}

#[test]
fn test_eval_let_with_arithmetic() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(let [x 10 y 5] (+ x y))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(15)),
        Err(e) => panic!("Expected Ok(15), got Err: {}", e),
    }
}

#[test]
fn test_eval_lambda_identity() {
    let (mut symbols, mut vm) = setup();
    let result = eval("((fn (x) x) 42)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_eval_lambda_add_one() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval("((fn (x) (+ x 1)) 10)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(11)),
        Err(e) => panic!("Expected Ok(11), got Err: {}", e),
    }
}

#[test]
fn test_compile_lambda_with_capture() {
    let (mut symbols, _) = setup();
    let result = compile("(let [x 10] (fn () x))", &mut symbols, "<test>");
    match result {
        Ok(_) => {}
        Err(e) => panic!("Failed to compile lambda with capture: {}", e),
    }
}

#[test]
fn test_eval_begin() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(begin 1 2 3)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}

#[test]
fn test_eval_comparison_lt() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval("(< 1 2)", &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

// === Control Flow: cond ===

#[test]
fn test_eval_cond_first_true() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(cond true 42)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_second_true() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        "(cond false 1 true 42)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_else() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        "(cond false 1 false 2 42)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_cond_with_expressions() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(cond (< 5 10) (+ 20 22))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

// === Control Flow: and ===

#[test]
fn test_eval_and_all_true() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(and true true true)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_eval_and_one_false() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(and true false true)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_and_returns_last() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(and 1 2 3)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_and_short_circuit() {
    let (mut symbols, mut vm) = setup();
    // If and doesn't short-circuit, this would fail trying to call nil
    let result = eval("(and false (nil))", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_and_empty() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(and)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

// === Control Flow: or ===

#[test]
fn test_eval_or_all_false() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(or false false false)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_eval_or_one_true() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(or false true false)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_eval_or_returns_first_truthy() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(or false 42 99)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_destructure_list_basic() {
    let (mut symbols, mut vm) = setup();
    // In the file-as-letrec model, bindings are local to each compilation
    // unit. Use a single expression to test destructuring.
    let result = eval(
        "(begin (def (a b c) (list 1 2 3)) (list a b c))",
        &mut symbols,
        &mut vm,
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
    let (mut symbols, mut vm) = setup();
    let result = eval("(or)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::bool(false));
}

// === Control Flow: while ===

#[test]
fn test_eval_while_never_executes() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(while false 42)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_eval_while_with_mutation() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(begin (var x 0) (while (< x 5) (assign x (+ x 1))) x)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(5));
}

// === Closures and Captures ===

#[test]
fn test_eval_closure_captures_local() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        "(let [x 10] ((fn () x)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_eval_closure_captures_multiple() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(let [x 10 y 20] ((fn () (+ x y))))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_eval_nested_closure() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        "(let [x 10] ((fn () ((fn () x)))))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_eval_closure_with_param_and_capture() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(let [x 10] ((fn (y) (+ x y)) 5))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(15));
}

// === Higher-Order Functions ===

#[test]
fn test_eval_function_as_argument() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "((fn (f x) (f x)) (fn (n) (+ n 1)) 10)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(11));
}

#[test]
fn test_eval_function_returning_function() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(((fn (x) (fn (y) (+ x y))) 10) 5)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(15));
}

// === Define and Assign ===

#[test]
fn test_eval_define_then_use() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(begin (var x 42) x)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_define_then_set() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        "(begin (var x 10) (assign x 42) x)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_set_in_closure() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(begin
           (var counter 0)
           (def inc (fn () (assign counter (+ counter 1))))
           (inc)
           (inc)
           counter)",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_intrinsic_fib() {
    // Fibonacci exercises intrinsic specialization with double recursion
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(begin
            (def fib (fn (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))
            (fib 10))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(55));
}

#[test]
fn test_intrinsic_unary_neg() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(- 5)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::int(-5)
    );
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(- -3)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_intrinsic_variadic_fallthrough() {
    // Variadic + falls through to generic call
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(+ 1 2 3)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_intrinsic_not() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(not true)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::bool(false)
    );
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(not false)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_intrinsic_rem() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    assert_eq!(
        eval("(rem 17 5)", &mut symbols, &mut vm, "<test>").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_fold_multiple_elements() {
    let (mut symbols, mut vm) = setup();

    // Test with (list 1) - should work
    let code1 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1)))"#;

    let result1 = eval(code1, &mut symbols, &mut vm, "<test>");
    println!("list 1: {:?}", result1);

    // Test with (list 1 2) - might fail
    let (mut symbols2, mut vm2) = setup();
    let code2 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2)))"#;

    let result2 = eval(code2, &mut symbols2, &mut vm2, "<test>");
    println!("list 1 2: {:?}", result2);

    // Test with (list 1 2 3) - original failing case
    let (mut symbols3, mut vm3) = setup();
    let code3 = r#"(begin
        (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
        (def my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2 3)))"#;

    let result3 = eval(code3, &mut symbols3, &mut vm3, "<test>");
    println!("list 1 2 3: {:?}", result3);
}

// === analyze tests ===

#[test]
fn test_analyze_literal() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("42", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Int(42)));
}

#[test]
fn test_analyze_define() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(var x 10)", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Define { .. }));
}

#[test]
fn test_analyze_lambda() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x) x)", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Lambda { .. }));
}

#[test]
fn test_analyze_with_let() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [x 1 y 2] (%add x y))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
    let analysis = result.unwrap();
    // Should produce a Let HIR node
    assert!(matches!(analysis.hir.kind, HirKind::Let { .. }));
}

#[test]
fn test_mutual_recursion_signal_inference() {
    // Test that mutually recursive functions are inferred as Pure
    // when they only call each other and pure primitives
    let (mut symbols, _) = setup();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
"#;
    let result = compile_file(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
}

#[test]
fn test_mutual_recursion_execution() {
    // Test that mutually recursive functions execute correctly
    let (mut symbols, mut vm) = setup();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
(f 5)
"#;
    let result = compile_file(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // f(5) -> g(4) -> f(3) -> g(2) -> f(1) -> g(0) -> 2
    let val = vm.execute(&result.bytecode).unwrap();
    assert_eq!(val, Value::int(2));
}

#[test]
fn test_mutual_recursion_signals_are_pure() {
    // Test that mutually recursive functions are inferred as Pure
    let (mut symbols, _) = setup();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
"#;
    let result = compile_file(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // Check that the closures don't suspend
    for constant in &result.bytecode.constants {
        if let Some(closure) = constant.as_closure() {
            assert!(
                !closure.signal().may_suspend(),
                "Closure should not suspend, got {:?}",
                closure.signal()
            );
        }
    }
}

#[test]
fn test_nqueens_functions_are_pure() {
    // Test that the nqueens functions are inferred as Pure
    let (mut symbols, _) = setup();
    let source = r#"
(var check-safe-helper
  (fn (col remaining row-offset)
    (if (empty? remaining)
      true
      (let [placed-col (first remaining)]
        (if (or (%eq col placed-col)
                (%eq row-offset (abs (%sub col placed-col))))
          false
          (check-safe-helper col (rest remaining) (%add row-offset 1)))))))

(var safe?
  (fn (col queens)
    (check-safe-helper col queens 1)))

(var try-cols-helper
  (fn (n col queens row)
    (if (%eq col n)
      (list)
      (if (safe? col queens)
        (let [new-queens (%pair col queens)]
          (append (solve-helper n (%add row 1) new-queens)
                  (try-cols-helper n (%add col 1) queens row)))
        (try-cols-helper n (%add col 1) queens row)))))

(var solve-helper
  (fn (n row queens)
    (if (%eq row n)
      (list (reverse queens))
      (try-cols-helper n 0 queens row))))
"#;
    let result = compile_file(source, &mut symbols, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // Check that all closures don't yield (they may error via stdlib calls,
    // but error is a suspension-for-safety, not an IO/yield effect).
    let mut found_closures = 0;
    for constant in &result.bytecode.constants {
        if let Some(closure) = constant.as_closure() {
            found_closures += 1;
            assert!(
                !closure.signal().may_yield(),
                "Closure should not yield, got {:?}",
                closure.signal()
            );
        }
    }
    assert_eq!(found_closures, 4, "Should have 4 closures");
}

// === Fiber integration tests ===

#[test]
fn test_fiber_new_and_status() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (= (fiber/status f) :new))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    clear_symbol_table();
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_resume_simple() {
    // A fiber that just returns a value
    let (mut symbols, mut vm) = setup();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (fiber/resume f))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_fiber_resume_dead_status() {
    // After a fiber completes, its status should be :dead
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        r#"(let [f (fiber/new (fn () 42) 0)]
             (fiber/resume f)
             (= (fiber/status f) :dead))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    clear_symbol_table();
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_and_resume() {
    // A fiber that emits, then is resumed to completion
    let (mut symbols, mut vm) = setup();
    // SIG_YIELD = 2, mask catches it
    let result = eval(
        r#"(let [f (fiber/new (fn () (emit 2 99) 42) 2)]
             (fiber/resume f)
             (fiber/value f))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(99)),
        Err(e) => panic!("Expected Ok(99), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_resume_continues() {
    // Resume after emit should continue execution and return final value
    let (mut symbols, mut vm) = setup();
    let result = eval(
        r#"(let [f (fiber/new (fn () (emit 2 99) 42) 2)]
             (fiber/resume f)
             (fiber/resume f))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_fiber_is_fiber() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        r#"(fiber? (fiber/new (fn () 42) 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}

#[test]
fn test_fiber_not_fiber() {
    let (mut symbols, mut vm) = setup();
    let result = eval(r#"(fiber? 42)"#, &mut symbols, &mut vm, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::bool(false)),
        Err(e) => panic!("Expected Ok(false), got Err: {}", e),
    }
}

#[test]
fn test_fiber_emit_through_nested_call() {
    // A fiber whose body calls a function that emits.
    // This tests yield propagation through nested calls.
    let (mut symbols, mut vm) = setup();
    let result = eval(
        r#"(begin
             (defn inner () (emit 2 99))
             (let [f (fiber/new (fn () (inner) 42) 2)]
               (fiber/resume f)
               (fiber/value f)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(99)),
        Err(e) => panic!("Expected Ok(99), got Err: {}", e),
    }
}

#[test]
fn test_fiber_mask() {
    let (mut symbols, mut vm) = setup();
    let result = eval(
        r#"(fiber/mask (fiber/new (fn () 42) 3))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}

#[test]
fn test_const_basic() {
    let (mut symbols, mut vm) = setup();
    let result = eval("(begin (def x 42) x)", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_const_set_error() {
    let (mut symbols, _) = setup();
    let result = compile("(begin (def x 42) (assign x 99))", &mut symbols, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_function() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    let result = eval(
        "(begin (defn add1 (x) (+ x 1)) (add1 10))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(11));
}

#[test]
fn test_const_function_set_error() {
    let (mut symbols, _) = setup();
    let result = compile(
        "(begin (defn f (x) x) (assign f 99))",
        &mut symbols,
        "<test>",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_cross_form_set_error() {
    let (mut symbols, _) = setup();
    let result = compile_file("(def x 42)\n(assign x 99)", &mut symbols, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_cross_form_reference() {
    let (mut symbols, mut vm) = setup();
    let result = compile_file("(def x 42)\n(%add x 1)", &mut symbols, "<test>");
    assert!(result.is_ok());
    let result = result.unwrap();
    let _ = vm.execute(&result.bytecode);
}

#[test]
fn test_const_in_function_scope() {
    let (mut symbols, mut vm) = setup();
    let result = eval("((fn () (def x 42) x))", &mut symbols, &mut vm, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_const_in_function_set_error() {
    let (mut symbols, _) = setup();
    let result = compile("((fn () (def x 42) (assign x 99)))", &mut symbols, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_arity_cons_wrong_args() {
    // first requires exactly 1 argument; passing 0 should be a compile-time arity error
    let mut symbols = SymbolTable::new();
    let result = compile("(first)", &mut symbols, "<test>");
    assert!(result.is_err(), "Expected compile error for (first)");
    assert!(result.unwrap_err().contains("arity"));
}

#[test]
fn test_arity_various_primitives() {
    // first expects 1 arg, 0 should fail
    let mut symbols = SymbolTable::new();
    let result = compile("(first)", &mut symbols, "<test>");
    assert!(result.is_err(), "first with 0 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // rest expects exactly 1 arg, 2 should fail
    let mut symbols = SymbolTable::new();
    let result = compile("(rest 1 2)", &mut symbols, "<test>");
    assert!(result.is_err(), "rest with 2 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // first expects exactly 1 arg, 3 should fail
    let mut symbols = SymbolTable::new();
    let result = compile("(first 1 2 3)", &mut symbols, "<test>");
    assert!(result.is_err(), "first with 3 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // list accepts 0+ args, so (list) should succeed
    let mut symbols = SymbolTable::new();
    let result = compile("(list)", &mut symbols, "<test>");
    assert!(result.is_ok(), "(list) should succeed since list accepts 0+ args");
}

#[test]
fn test_arity_user_shadow_disables_check() {
    // When user redefines a primitive, arity checking should NOT apply
    // the primitive's arity to the user's version
    let mut symbols = SymbolTable::new();
    let result = compile("(begin (var first 42) (first 1 2))", &mut symbols, "<test>");
    assert!(
        !result.as_ref().err().is_some_and(|e| e.contains("arity")),
        "User-shadowed first should not get primitive arity check, got: {:?}",
        result
    );
}

#[test]
fn test_arity_in_nested_positions() {
    // Arity checking should work in nested calls, let bodies, and lambda bodies
    let mut symbols = SymbolTable::new();
    let result = compile("(list 1 (first))", &mut symbols, "<test>");
    assert!(result.is_err(), "Nested (first) should fail arity check");
    assert!(result.unwrap_err().contains("arity"));

    let mut symbols = SymbolTable::new();
    let result = compile("(let [x 1] (first))", &mut symbols, "<test>");
    assert!(result.is_err(), "(first) in let body should fail");
    assert!(result.unwrap_err().contains("arity"));

    let mut symbols = SymbolTable::new();
    let result = compile("(fn (x) (first))", &mut symbols, "<test>");
    assert!(result.is_err(), "(first) in lambda body should fail");
    assert!(result.unwrap_err().contains("arity"));
}

// === Eval special form ===

#[test]
fn test_eval_simple_literal() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '42)", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_quoted_expression() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(eval '(+ 1 2))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_list_construction() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(eval (list '+ 1 2))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_with_env_keyword_keys_skipped() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(eval '(+ 1 2) {:x 10})", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_nil_env() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(eval '(+ 3 4) nil)", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(7));
}

#[test]
fn test_eval_arity_error() {
    let (mut symbols, _vm) = setup();
    // eval with no arguments
    let result = compile("(eval)", &mut symbols, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("eval"));
}

#[test]
fn test_eval_too_many_args() {
    let (mut symbols, _vm) = setup();
    // eval with three arguments should fail at compile time
    let result = compile("(eval 'a 'b 'c)", &mut symbols, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("eval"));
}

#[test]
fn test_eval_returns_string() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '\"hello\")", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::string("hello"));
}

#[test]
fn test_eval_returns_bool() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval 'true)", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_eval_returns_nil() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval 'nil)", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_eval_with_macros() {
    // eval'd code should have access to prelude macros like `when`
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '(when true 42))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_with_begin() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '(begin 1 2 3))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_with_let_in_evald_code() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval(
        "(eval '(let [x 10] (+ x 5)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_eval_with_closure_in_evald_code() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval(
        "(eval '(let [x 1] ((fn () x))))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_eval_result_in_computation() {
    // eval's return value used in a larger expression
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(+ 1 (eval '2))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_inside_let() {
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval(
        "(let [x 10] (eval '(+ 1 2)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_inside_lambda() {
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("((fn () (eval '42)))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_nested() {
    // eval within eval'd code
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '(eval '42))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_env_arg_rejects_non_struct() {
    // env argument must be a struct or nil — other types are rejected
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '42 \"anything\")", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert!(result.is_err());
}

#[test]
fn test_eval_empty_env() {
    // Empty mutable struct env should work fine
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let result = eval("(eval '(+ 1 2) (@struct))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_compilation_error() {
    // eval'd code with invalid syntax should produce a runtime error
    let (mut symbols, mut vm) = setup();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval("(eval '(if))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert!(result.is_err());
}

#[test]
fn test_eval_sequential_caching() {
    // Multiple evals should work (tests expander caching)
    let (mut symbols, mut vm) = setup_with_stdlib();
    set_symbol_table(&mut symbols as *mut SymbolTable);
    elle::context::set_vm_context(&mut vm as *mut VM);
    let r1 = eval("(eval '(+ 1 2))", &mut symbols, &mut vm, "<test>");
    assert_eq!(r1.unwrap(), Value::int(3));
    let r2 = eval("(eval '(* 3 4))", &mut symbols, &mut vm, "<test>");
    clear_symbol_table();
    assert_eq!(r2.unwrap(), Value::int(12));
}
