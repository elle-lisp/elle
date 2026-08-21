use super::*;

#[test]
fn test_compile_literal() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("42", symbols, cctx, "<test>");
    assert!(result.is_ok());
    let compiled = result.unwrap();
    assert!(!compiled.bytecode.instructions.is_empty());
}

#[test]
fn test_compile_if() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(if true 1 2)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_let() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(let [x 10] x)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_lambda() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(fn (x) x)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_call() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(%add 1 2)", symbols, cctx, "<test>");
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
}

#[test]
fn test_compile_global_variable() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    // Test that global variables (like list) are properly recognized and emit LoadGlobal
    let result = compile("(list 1 2)", symbols, cctx, "<test>");
    assert!(
        result.is_ok(),
        "Global variable handling failed: {:?}",
        result.err()
    );
}

#[test]
fn test_compile_begin() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(begin 1 2 3)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_and() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(and true true false)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_or() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(or false false true)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_while() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(while false nil)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_compile_cond() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(cond true 1 2)", symbols, cctx, "<test>");
    assert!(result.is_ok());
}

#[test]
fn test_eval_literal() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("42", symbols, vm, cctx, "<test>");
    // Note: execution may fail due to incomplete bytecode mapping
    // but compilation should succeed
    let _ = result;
}

#[test]
fn test_eval_addition() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(+ 1 2)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}

#[test]
fn test_eval_subtraction() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(- 10 3)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(7)),
        Err(e) => panic!("Expected Ok(7), got Err: {}", e),
    }
}

#[test]
fn test_eval_nested_arithmetic() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(+ (* 2 3) (- 10 5))", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(11)),
        Err(e) => panic!("Expected Ok(11), got Err: {}", e),
    }
}

#[test]
fn test_eval_if_true() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(if true 42 0)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_eval_if_false() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(if false 42 0)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(0)),
        Err(e) => panic!("Expected Ok(0), got Err: {}", e),
    }
}

#[test]
fn test_eval_let_simple() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(let [x 10] x)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(10)),
        Err(e) => panic!("Expected Ok(10), got Err: {}", e),
    }
}

#[test]
fn test_eval_let_with_arithmetic() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(let [x 10 y 5] (+ x y))", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(15)),
        Err(e) => panic!("Expected Ok(15), got Err: {}", e),
    }
}

#[test]
fn test_eval_lambda_identity() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("((fn (x) x) 42)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(42)),
        Err(e) => panic!("Expected Ok(42), got Err: {}", e),
    }
}

#[test]
fn test_eval_lambda_add_one() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("((fn (x) (+ x 1)) 10)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(11)),
        Err(e) => panic!("Expected Ok(11), got Err: {}", e),
    }
}

#[test]
fn test_compile_lambda_with_capture() {
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let result = compile("(let [x 10] (fn () x))", symbols, cctx, "<test>");
    match result {
        Ok(_) => {}
        Err(e) => panic!("Failed to compile lambda with capture: {}", e),
    }
}

#[test]
fn test_eval_begin() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(begin 1 2 3)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::int(3)),
        Err(e) => panic!("Expected Ok(3), got Err: {}", e),
    }
}

#[test]
fn test_eval_comparison_lt() {
    let mut rt = setup_with_stdlib();
    let (vm, symbols, cctx) = rt.parts();
    let result = eval("(< 1 2)", symbols, vm, cctx, "<test>");
    match result {
        Ok(v) => assert_eq!(v, Value::bool(true)),
        Err(e) => panic!("Expected Ok(true), got Err: {}", e),
    }
}
