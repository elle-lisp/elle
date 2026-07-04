use super::*;

#[test]
fn test_const_basic() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(begin (def x 42) x)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_const_set_error() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(begin (def x 42) (assign x 99))", symbols, cctx, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_function() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(begin (defn add1 (x) (+ x 1)) (add1 10))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(11));
}

#[test]
fn test_const_function_set_error() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile(
        "(begin (defn f (x) x) (assign f 99))",
        symbols,
        cctx,
        "<test>",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_cross_form_set_error() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile_file("(def x 42)\n(assign x 99)", symbols, cctx, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_const_cross_form_reference() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = compile_file("(def x 42)\n(%add x 1)", symbols, cctx, "<test>");
    assert!(result.is_ok());
    let result = result.unwrap();
    let _ = vm.execute(&result.bytecode);
}

#[test]
fn test_const_in_function_scope() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("((fn () (def x 42) x))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_const_in_function_set_error() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile(
        "((fn () (def x 42) (assign x 99)))",
        symbols,
        cctx,
        "<test>",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

#[test]
fn test_arity_cons_wrong_args() {
    // first requires exactly 1 argument; passing 0 should be a compile-time arity error
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(first)", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "Expected compile error for (first)");
    assert!(result.unwrap_err().contains("arity"));
}

#[test]
fn test_arity_various_primitives() {
    // first expects 1 arg, 0 should fail
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(first)", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "first with 0 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // rest expects exactly 1 arg, 2 should fail
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(rest 1 2)", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "rest with 2 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // first expects exactly 1 arg, 3 should fail
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(first 1 2 3)", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "first with 3 args should fail");
    assert!(result.unwrap_err().contains("arity"));

    // list accepts 0+ args, so (list) should succeed
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(list)", &mut symbols, &mut cctx, "<test>");
    assert!(
        result.is_ok(),
        "(list) should succeed since list accepts 0+ args"
    );
}

#[test]
fn test_arity_user_shadow_disables_check() {
    // When user redefines a primitive, arity checking should NOT apply
    // the primitive's arity to the user's version
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile(
        "(begin (var first 42) (first 1 2))",
        &mut symbols,
        &mut cctx,
        "<test>",
    );
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
    let mut cctx = CompileCtx::new();
    let result = compile("(list 1 (first))", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "Nested (first) should fail arity check");
    assert!(result.unwrap_err().contains("arity"));

    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(let [x 1] (first))", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "(first) in let body should fail");
    assert!(result.unwrap_err().contains("arity"));

    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    let result = compile("(fn (x) (first))", &mut symbols, &mut cctx, "<test>");
    assert!(result.is_err(), "(first) in lambda body should fail");
    assert!(result.unwrap_err().contains("arity"));
}

// === Eval special form ===

#[test]
fn test_eval_simple_literal() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '42)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_quoted_expression() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(+ 1 2))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_list_construction() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval (list '+ 1 2))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_with_env_keyword_keys_skipped() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(+ 1 2) {:x 10})", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_nil_env() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(+ 3 4) nil)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(7));
}

#[test]
fn test_eval_arity_error() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    // eval with no arguments
    let result = compile("(eval)", symbols, cctx, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("eval"));
}

#[test]
fn test_eval_too_many_args() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    // eval with three arguments should fail at compile time
    let result = compile("(eval 'a 'b 'c)", symbols, cctx, "<test>");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("eval"));
}

#[test]
fn test_eval_returns_string() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '\"hello\")", symbols, vm, cctx, "<test>");
    // The comparison `Value::string` allocates; build it through a test ctx so it
    // is born in an explicit region (named through the test ctx). The assert
    // runs INSIDE the closure so the comparison value outlives `with_test_ctx`'s
    // region teardown.
    elle::primitives::ctx::with_test_ctx(|ctx| {
        assert_eq!(result.unwrap(), ctx.string("hello"));
    });
}

#[test]
fn test_eval_returns_bool() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval 'true)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_eval_returns_nil() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval 'nil)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_eval_with_macros() {
    // eval'd code should have access to prelude macros like `when`
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(when true 42))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_with_begin() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(begin 1 2 3))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_with_let_in_evald_code() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(let [x 10] (+ x 5)))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_eval_with_closure_in_evald_code() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval(
        "(eval '(let [x 1] ((fn () x))))",
        symbols,
        vm,
        cctx,
        "<test>",
    );
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_eval_result_in_computation() {
    // eval's return value used in a larger expression
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(+ 1 (eval '2))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_inside_let() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(let [x 10] (eval '(+ 1 2)))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_inside_lambda() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("((fn () (eval '42)))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_nested() {
    // eval within eval'd code
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(eval '42))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_eval_env_arg_rejects_non_struct() {
    // env argument must be a struct or nil — other types are rejected
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '42 \"anything\")", symbols, vm, cctx, "<test>");
    assert!(result.is_err());
}

#[test]
fn test_eval_empty_env() {
    // Empty mutable struct env should work fine
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(+ 1 2) (@struct))", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_eval_compilation_error() {
    // eval'd code with invalid syntax should produce a runtime error
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(eval '(if))", symbols, vm, cctx, "<test>");
    assert!(result.is_err());
}

#[test]
fn test_eval_sequential_caching() {
    // Multiple evals should work (tests expander caching)
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let r1 = eval("(eval '(+ 1 2))", symbols, vm, cctx, "<test>");
    assert_eq!(r1.unwrap(), Value::int(3));
    let r2 = eval("(eval '(* 3 4))", symbols, vm, cctx, "<test>");
    assert_eq!(r2.unwrap(), Value::int(12));
}
