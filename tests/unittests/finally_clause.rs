use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

struct FinallyEval;

impl FinallyEval {
    fn eval(code: &str) -> Result<Value, String> {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let value = read_str(code, &mut symbols)?;
        let expr = value_to_expr(&value, &mut symbols)?;
        let bytecode = compile(&expr);
        vm.execute(&bytecode)
    }
}

// ============================================================================
// Parsing Tests - Verify finally clause syntax
// ============================================================================

#[test]
fn unit_finally_clause_parses() {
    // Verify finally clause parses correctly
    let code = "(try 42 (finally 0))";
    let result = FinallyEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_finally_with_expression_parses() {
    // Verify finally with expression parses
    let code = "(try (+ 1 2) (finally (+ 3 4)))";
    let result = FinallyEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_finally_with_catch_and_finally_parses() {
    // Verify try/catch/finally syntax
    let code = "(try 42 (catch e 0) (finally 1))";
    let result = FinallyEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_nested_finally_parses() {
    // Verify nested finally blocks parse
    let code = "(try (try 1 (finally 2)) (finally 3))";
    let result = FinallyEval::eval(code);
    assert!(result.is_ok());
}

// ============================================================================
// Execution Tests - Verify finally block behavior
// ============================================================================

#[test]
fn unit_finally_returns_try_value() {
    // Finally block should NOT change the return value of try
    let result = FinallyEval::eval("(try 42 (finally 999))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn unit_finally_with_arithmetic_returns_try_value() {
    // Finally's computation result should be discarded
    let result = FinallyEval::eval("(try 100 (finally (+ 50 50)))").unwrap();
    assert_eq!(result, Value::Int(100)); // Not 100
}

#[test]
fn unit_finally_returns_string_value() {
    // Finally should preserve string returns
    let result = FinallyEval::eval("(try \"hello\" (finally 0))").unwrap();
    assert_eq!(result, Value::String("hello".into()));
}

#[test]
fn unit_finally_returns_list_value() {
    // Finally should preserve list returns
    let result = FinallyEval::eval("(try (list 1 2 3) (finally nil))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn unit_finally_with_nil_try() {
    // Finally should preserve nil returns
    let result = FinallyEval::eval("(try nil (finally 99))").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn unit_finally_with_boolean() {
    // Finally should preserve boolean returns
    let result = FinallyEval::eval("(try #t (finally #f))").unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn unit_finally_executes_with_side_effects() {
    // Verify finally block is compiled and executed
    // (We can't directly observe output, but we verify it doesn't error)
    let result = FinallyEval::eval("(try 42 (finally (display \"cleanup\")))");
    assert!(result.is_ok());
}

// NOTE: Variable reference test skipped due to Issue #6 (local variable binding not implemented)
// #[test]
// fn unit_finally_with_variable_reference() {
//     // Finally can reference variables in scope
//     let code = "(let ((x 10)) (try x (finally (+ x 5))))";
//     let result = FinallyEval::eval(code).unwrap();
//     assert_eq!(result, Value::Int(10)); // Returns try body value, not finally
// }

#[test]
fn unit_nested_finally_inner_returns() {
    // Inner try returns through outer finally
    let code = "(try (try 5 (finally 10)) (finally 20))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(5)); // Inner try value
}

#[test]
fn unit_finally_with_list_operations() {
    // Finally can contain list operations
    let code = "(try (list 1 2) (finally (list 3 4 5)))";
    let result = FinallyEval::eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2); // From try, not finally
}

#[test]
fn unit_finally_with_arithmetic_operations() {
    // Finally with complex arithmetic
    let code = "(try 100 (finally (* (+ 2 3) 10)))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(100)); // From try
}

#[test]
fn unit_finally_multiple_expressions() {
    // Finally executes all expressions
    let code = "(try 42 (finally (begin (+ 1 1) (- 5 2) 0)))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(42)); // From try
}

#[test]
fn unit_finally_with_comparison() {
    // Finally can use comparison operators
    let code = "(try 7 (finally (> 10 5)))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(7)); // From try
}

#[test]
fn unit_finally_preserves_all_types() {
    // Verify finally preserves each type correctly
    let int_code = "(try 42 (finally 0))";
    let string_code = "(try \"test\" (finally \"\"))";
    let bool_code = "(try #t (finally #f))";
    let nil_code = "(try nil (finally 1))";

    assert_eq!(FinallyEval::eval(int_code).unwrap(), Value::Int(42));
    assert_eq!(
        FinallyEval::eval(string_code).unwrap(),
        Value::String("test".into())
    );
    assert_eq!(FinallyEval::eval(bool_code).unwrap(), Value::Bool(true));
    assert_eq!(FinallyEval::eval(nil_code).unwrap(), Value::Nil);
}

#[test]
fn unit_finally_with_catch_clause() {
    // Finally executes alongside catch clause
    let code = "(try 50 (catch e 0) (finally 100))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(50)); // From try
}

#[test]
fn unit_deeply_nested_finally() {
    // Multiple levels of nesting
    let code = "(try (try (try 5 (finally 6)) (finally 7)) (finally 8))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(5)); // From innermost try
}

#[test]
fn unit_finally_with_conditional() {
    // Finally with if expression
    let code = "(try 100 (finally (if #t 1 2)))";
    let result = FinallyEval::eval(code).unwrap();
    assert_eq!(result, Value::Int(100)); // From try
}

#[test]
fn unit_finally_value_discarded() {
    // Explicitly verify finally's value is discarded
    let code = "(try 999 (finally 111))";
    let result = FinallyEval::eval(code).unwrap();
    // If finally's value was used, result would be 111
    // But it should be 999 from try block
    assert_eq!(result, Value::Int(999));
}
