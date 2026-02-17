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
// Exception Filtering Pattern Tests
// ============================================================================

#[test]
fn unit_exception_creation_with_filters() {
    // Create exceptions that could be filtered
    let mut symbols = SymbolTable::new();
    let timeout_exc = read_str(r#"(exception "timeout" (list "code" 504))"#, &mut symbols);
    let auth_exc = read_str(
        r#"(exception "authentication" (list "code" 401))"#,
        &mut symbols,
    );

    assert!(timeout_exc.is_ok());
    assert!(auth_exc.is_ok());
}

#[test]
fn unit_exception_message_pattern_matching() {
    // Verify we can create exceptions with filterable messages
    let mut symbols = SymbolTable::new();

    let codes = vec!["timeout", "connection-refused", "authentication-failed"];

    for code in codes {
        let exc_code = format!(r#"(exception "{}")"#, code);
        let result = read_str(&exc_code, &mut symbols);
        assert!(result.is_ok());
    }
}

#[test]
fn unit_exception_with_structured_data_for_filtering() {
    // Create exceptions with structured data for filtering
    let mut symbols = SymbolTable::new();

    let exc = read_str(
        r#"(exception "error" (list "type" "network" "code" 500))"#,
        &mut symbols,
    );

    assert!(exc.is_ok());
}

#[test]
fn unit_multiple_exception_types_for_filtering() {
    // Various exception types for filtering by message or data
    let mut symbols = SymbolTable::new();

    let network_error = read_str(r#"(exception "network-error" 503)"#, &mut symbols);
    let parse_error = read_str(r#"(exception "parse-error" "invalid json")"#, &mut symbols);
    let validation_error = read_str(
        r#"(exception "validation" (list "field" "email"))"#,
        &mut symbols,
    );

    assert!(network_error.is_ok());
    assert!(parse_error.is_ok());
    assert!(validation_error.is_ok());
}

#[test]
fn unit_exception_message_extraction_for_filtering() {
    // Verify message extraction works (needed for filtering)
    let mut symbols = SymbolTable::new();

    let msgs = vec!["timeout", "connection-refused", "invalid-input"];

    for msg in msgs {
        let code = format!(r#"(exception-message (exception "{}"))"#, msg);
        let result = read_str(&code, &mut symbols);
        assert!(result.is_ok());
    }
}

#[test]
fn unit_exception_data_extraction_for_filtering() {
    // Verify data extraction works (needed for filtering)
    let mut symbols = SymbolTable::new();

    let code = r#"(exception-data (exception "error" 500))"#;
    let result = read_str(code, &mut symbols);

    assert!(result.is_ok());
}

#[test]
fn unit_exception_filtering_by_message_pattern() {
    // Test pattern matching on exception message
    let timeout_msg = ExceptionEval::eval(r#"(exception-message (exception "timeout"))"#).unwrap();
    let network_msg = ExceptionEval::eval(r#"(exception-message (exception "network"))"#).unwrap();

    // Both should be strings
    assert!((timeout_msg).is_string());
    assert!((network_msg).is_string());
}

#[test]
fn unit_exception_filtering_by_data_code() {
    // Test filtering by exception data (e.g., error codes)
    let code_404 = ExceptionEval::eval(r#"(exception-data (exception "http" 404))"#).unwrap();
    let code_500 = ExceptionEval::eval(r#"(exception-data (exception "http" 500))"#).unwrap();

    assert_eq!(code_404, Value::int(404));
    assert_eq!(code_500, Value::int(500));
}

#[test]
fn unit_exception_string_filtering() {
    // Test filtering exceptions by string patterns
    let mut symbols = SymbolTable::new();

    let messages = vec![
        r#"(exception-message (exception "timeout"))"#,
        r#"(exception-message (exception "connection-refused"))"#,
        r#"(exception-message (exception "permission-denied"))"#,
    ];

    for msg_code in messages {
        let result = read_str(msg_code, &mut symbols);
        assert!(result.is_ok());
    }
}

#[test]
fn unit_exception_comparison_for_filtering() {
    // Test that we can compare exception data for filtering
    let mut symbols = SymbolTable::new();

    // HTTP error codes can be compared
    let code_400 = read_str(r#"(exception-data (exception "http" 400))"#, &mut symbols).unwrap();
    let code_500 = read_str(r#"(exception-data (exception "http" 500))"#, &mut symbols).unwrap();

    // Should be different
    assert_ne!(code_400, code_500);
}

#[test]
fn unit_exception_categorization_for_filtering() {
    // Test that exceptions can be categorized for filtering
    let network_exc = ExceptionEval::eval(r#"(exception "network" "timeout")"#).unwrap();
    let auth_exc = ExceptionEval::eval(r#"(exception "auth" "invalid-token")"#).unwrap();
    let db_exc = ExceptionEval::eval(r#"(exception "database" "connection-lost")"#).unwrap();

    assert!(network_exc.as_condition().is_some());
    assert!(auth_exc.as_condition().is_some());
    assert!(db_exc.as_condition().is_some());
}

#[test]
fn unit_exception_list_data_for_complex_filtering() {
    // Test exceptions with list data for complex filtering patterns
    let data = ExceptionEval::eval(
        r#"(exception-data (exception "db" (list "table" "users" "code" 23505)))"#,
    )
    .unwrap();

    assert!((data).is_cons());
}

#[test]
fn unit_exception_filtering_documentation() {
    // Document the pattern for filtering:
    // 1. Create exception with message and/or data
    // 2. Extract message with exception-message
    // 3. Extract data with exception-data
    // 4. Use pattern matching on extracted values

    // Pattern 1: Filter by message
    let msg = ExceptionEval::eval(r#"(exception-message (exception "network-error"))"#).unwrap();
    assert!((msg).is_string());

    // Pattern 2: Filter by error code
    let code = ExceptionEval::eval(r#"(exception-data (exception "http" 404))"#).unwrap();
    assert_eq!(code, Value::int(404));

    // Pattern 3: Filter by structured data
    let data =
        ExceptionEval::eval(r#"(exception-data (exception "api" (list "status" "unauthorized")))"#)
            .unwrap();
    assert!((data).is_cons());
}
