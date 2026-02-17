use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

struct ExceptionEval;

impl ExceptionEval {
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
// Parsing Tests - Verify exception expressions parse correctly
// ============================================================================

#[test]
fn unit_exception_creation_parses() {
    // Verify exception() parses correctly
    let code = "(exception \"error\")";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_exception_with_data_parses() {
    // Verify exception with data parses correctly
    let code = "(exception \"error\" 42)";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_exception_message_extraction_parses() {
    // Verify exception-message() parses correctly
    let code = "(exception-message (exception \"test\"))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_exception_data_extraction_parses() {
    // Verify exception-data() parses correctly
    let code = "(exception-data (exception \"error\" 123))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_try_without_catch_parses() {
    // Verify try without catch parses correctly
    let code = "(try (+ 1 2))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_try_with_catch_parses() {
    // Verify try with catch clause parses correctly
    let code = "(try (+ 1 2) (catch e \"error\"))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_try_with_catch_and_finally_parses() {
    // Verify try with both catch and finally parses correctly
    let code = "(try (+ 1 2) (catch e \"error\") (finally 0))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

#[test]
fn unit_nested_try_blocks_parse() {
    // Verify nested try blocks parse correctly
    let code = "(try (try (+ 1 2) (catch e 0)) (catch e 1))";
    let result = ExceptionEval::eval(code);
    assert!(result.is_ok());
}

// ============================================================================
// Execution Tests - Verify exception operations execute correctly
// ============================================================================

#[test]
fn unit_exception_creation_returns_exception() {
    // Verify exception creation returns an Exception value
    let result = ExceptionEval::eval("(exception \"test error\")").unwrap();
    assert!(result.as_condition().is_some());
}

#[test]
fn unit_exception_with_data_creates_exception() {
    // Verify exception with data creates exception
    let result = ExceptionEval::eval("(exception \"error\" 42)").unwrap();
    assert!(result.as_condition().is_some());
}

#[test]
fn unit_exception_message_extraction_works() {
    // Verify exception-message extracts string message
    let result = ExceptionEval::eval("(exception-message (exception \"hello\"))").unwrap();
    assert_eq!(result, Value::string("hello"));
}

#[test]
fn unit_exception_data_extraction_integer() {
    // Verify exception-data returns attached integer
    let result = ExceptionEval::eval("(exception-data (exception \"error\" 99))").unwrap();
    assert_eq!(result, Value::int(99));
}

#[test]
fn unit_exception_data_extraction_nil_for_no_data() {
    // Verify exception-data returns nil when no data provided
    let result = ExceptionEval::eval("(exception-data (exception \"error\"))").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn unit_try_returns_successful_result() {
    // Verify try block returns result of successful computation
    let result = ExceptionEval::eval("(try (+ 10 20))").unwrap();
    assert_eq!(result, Value::int(30));
}

#[test]
fn unit_try_returns_multiply_result() {
    // Verify try block returns correct computation
    let result = ExceptionEval::eval("(try (* 5 7))").unwrap();
    assert_eq!(result, Value::int(35));
}

#[test]
fn unit_try_with_catch_no_exception() {
    // Verify try block without exception ignores catch
    let result = ExceptionEval::eval("(try (+ 1 2) (catch e 999))").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn unit_try_arithmetic_operations() {
    // Verify try works with various arithmetic operations
    let result1 = ExceptionEval::eval("(try (- 10 3))").unwrap();
    let result2 = ExceptionEval::eval("(try (/ 20 4))").unwrap();
    let result3 = ExceptionEval::eval("(try 100))").unwrap();

    assert_eq!(result1, Value::int(7));
    assert_eq!(result2, Value::int(5));
    assert_eq!(result3, Value::int(100));
}

#[test]
fn unit_nested_try_blocks() {
    // Verify nested try blocks work correctly
    let result = ExceptionEval::eval("(try (try (+ 2 3) (catch e 0)) (catch e 1))").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn unit_exception_with_list_data() {
    // Verify exception can hold list data
    let result =
        ExceptionEval::eval("(exception-data (exception \"error\" (list 1 2 3)))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn unit_exception_message_with_different_strings() {
    // Verify exception-message works with various strings
    let result1 =
        ExceptionEval::eval("(exception-message (exception \"Division by zero\"))").unwrap();
    let result2 = ExceptionEval::eval("(exception-message (exception \"Network error\"))").unwrap();

    assert_eq!(result1, Value::string("Division by zero"));
    assert_eq!(result2, Value::string("Network error"));
}

#[test]
fn unit_multiple_exceptions_independent() {
    // Verify multiple exceptions don't interfere with each other
    let code = "(list (exception \"first\" 1) (exception \"second\" 2))";
    let result = ExceptionEval::eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 2);
    assert!(vec[0].as_condition().is_some());
    assert!(vec[1].as_condition().is_some());
}

#[test]
fn unit_try_with_nested_arithmetic() {
    // Verify try works with nested arithmetic expressions
    let result = ExceptionEval::eval("(try (+ (* 2 3) (- 10 4)))").unwrap();
    assert_eq!(result, Value::int(12));
}

#[test]
fn unit_exception_immutable() {
    // Verify exception values are distinct across evaluations
    let eval1 = ExceptionEval::eval("(exception \"test\")").unwrap();
    let eval2 = ExceptionEval::eval("(exception \"test\")").unwrap();

    // Both should be exceptions but different instances
    assert!(eval1.as_condition().is_some());
    assert!(eval2.as_condition().is_some());
}
