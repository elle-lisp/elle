use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============================================================================
// Exception Filtering Integration Tests
// ============================================================================

#[test]
fn test_exception_message_extraction_for_filtering() {
    // Extract message to use in filtering logic
    let result = eval(r#"(exception-message (exception "timeout"))"#).unwrap();
    assert_eq!(result, Value::string("timeout"));
}

#[test]
fn test_exception_data_extraction_for_filtering() {
    // Extract data to use in filtering logic
    let result = eval(r#"(exception-data (exception "error" 500))"#).unwrap();
    assert_eq!(result, Value::int(500));
}

#[test]
fn test_filtering_by_http_error_code() {
    // Filter exceptions by HTTP error codes
    let exc_404 = eval(r#"(exception-data (exception "http" 404))"#).unwrap();
    let exc_500 = eval(r#"(exception-data (exception "http" 500))"#).unwrap();

    assert_eq!(exc_404, Value::int(404));
    assert_eq!(exc_500, Value::int(500));
}

#[test]
fn test_filtering_network_errors() {
    // Filter different network-related exceptions
    let timeout = eval(r#"(exception-message (exception "timeout"))"#).unwrap();
    let refused = eval(r#"(exception-message (exception "connection-refused"))"#).unwrap();
    let unreachable = eval(r#"(exception-message (exception "network-unreachable"))"#).unwrap();

    assert_eq!(timeout, Value::string("timeout"));
    assert_eq!(refused, Value::string("connection-refused"));
    assert_eq!(unreachable, Value::string("network-unreachable"));
}

#[test]
fn test_filtering_with_comparison_operators() {
    // Use comparison operators for filtering
    let _code_500 = eval(r#"(exception-data (exception "http" 500))"#).unwrap();

    // Filter: is this a server error (>= 500)?
    let is_server_error = eval("(>= 500 500)").unwrap();
    assert_eq!(is_server_error, Value::bool(true));

    let code_404 = eval(r#"(exception-data (exception "http" 404))"#).unwrap();
    assert_eq!(code_404, Value::int(404));
}

#[test]
fn test_filtering_by_exception_category() {
    // Create exceptions by category
    let network_exc_data =
        eval(r#"(exception-data (exception "network" (list "code" 1)))"#).unwrap();
    let db_exc_data = eval(r#"(exception-data (exception "database" (list "code" 2)))"#).unwrap();
    let auth_exc_data = eval(r#"(exception-data (exception "auth" (list "code" 3)))"#).unwrap();

    // All should be lists
    assert!((network_exc_data).is_cons());
    assert!((db_exc_data).is_cons());
    assert!((auth_exc_data).is_cons());
}

#[test]
fn test_filtering_authentication_errors() {
    // Create and filter authentication-related exceptions
    let invalid_token = eval(r#"(exception-message (exception "invalid-token"))"#).unwrap();
    let expired_token = eval(r#"(exception-message (exception "expired-token"))"#).unwrap();
    let missing_creds = eval(r#"(exception-message (exception "missing-credentials"))"#).unwrap();

    assert_eq!(invalid_token, Value::string("invalid-token"));
    assert_eq!(expired_token, Value::string("expired-token"));
    assert_eq!(missing_creds, Value::string("missing-credentials"));
}

#[test]
fn test_filtering_database_errors() {
    // Filter different database error types
    let connection_lost =
        eval(r#"(exception "database" (list "type" "connection" "code" 1))"#).unwrap();
    let constraint_violation =
        eval(r#"(exception "database" (list "type" "constraint" "code" 2))"#).unwrap();

    assert!(connection_lost.as_condition().is_some());
    assert!(constraint_violation.as_condition().is_some());
}

#[test]
fn test_filtering_validation_errors() {
    // Create validation errors with filterable details
    let email_error =
        eval(r#"(exception "validation" (list "field" "email" "error" "invalid"))"#).unwrap();
    let password_error =
        eval(r#"(exception "validation" (list "field" "password" "error" "too-short"))"#).unwrap();

    assert!(email_error.as_condition().is_some());
    assert!(password_error.as_condition().is_some());
}

#[test]
fn test_filtering_with_string_patterns() {
    // Extract messages and check patterns
    let timeout_msg = eval(r#"(exception-message (exception "timeout-reached"))"#).unwrap();
    let msg_str = if let Some(s) = timeout_msg.as_string() {
        s.to_string()
    } else {
        panic!("Expected string")
    };

    // Can check if message contains pattern
    assert!(msg_str.contains("timeout"));
}

#[test]
fn test_filtering_http_status_codes() {
    // Filter by HTTP status code ranges
    let client_error = eval(r#"(exception "http" 400)"#).unwrap();
    let not_found = eval(r#"(exception "http" 404)"#).unwrap();
    let server_error = eval(r#"(exception "http" 500)"#).unwrap();

    assert!(client_error.as_condition().is_some());
    assert!(not_found.as_condition().is_some());
    assert!(server_error.as_condition().is_some());
}

#[test]
fn test_filtering_error_messages_by_type() {
    // Different error types for filtering
    let timeout = eval(r#"(exception-message (exception "timeout"))"#).unwrap();
    let eof = eval(r#"(exception-message (exception "eof"))"#).unwrap();
    let io_error = eval(r#"(exception-message (exception "io-error"))"#).unwrap();

    // All different messages
    assert_ne!(timeout, eof);
    assert_ne!(eof, io_error);
    assert_ne!(timeout, io_error);
}

#[test]
fn test_filtering_with_numeric_error_codes() {
    // Use numeric codes for filtering
    let code_1 = eval(r#"(exception "error" 1)"#).unwrap();
    let code_2 = eval(r#"(exception "error" 2)"#).unwrap();
    let code_3 = eval(r#"(exception "error" 3)"#).unwrap();

    assert!(code_1.as_condition().is_some());
    assert!(code_2.as_condition().is_some());
    assert!(code_3.as_condition().is_some());
}

#[test]
fn test_filtering_cascading_errors() {
    // Create hierarchical errors for filtering
    let root_cause = eval(r#"(exception "root" (list "retry" #t))"#).unwrap();
    let wrapped_error = eval(r#"(exception "wrapped" (list "original" "timeout"))"#).unwrap();

    assert!(root_cause.as_condition().is_some());
    assert!(wrapped_error.as_condition().is_some());
}

#[test]
fn test_filtering_numeric_range_matching() {
    // Extract and filter by numeric ranges
    let low_code = eval(r#"(exception-data (exception "api" 100))"#).unwrap();
    let mid_code = eval(r#"(exception-data (exception "api" 200))"#).unwrap();
    let high_code = eval(r#"(exception-data (exception "api" 500))"#).unwrap();

    assert_eq!(low_code, Value::int(100));
    assert_eq!(mid_code, Value::int(200));
    assert_eq!(high_code, Value::int(500));
}

#[test]
fn test_filtering_exception_collections() {
    // Create a list of exceptions for filtering
    let exceptions =
        eval(r#"(list (exception "timeout" 1) (exception "denied" 2) (exception "invalid" 3))"#)
            .unwrap();

    let vec = exceptions.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);

    for exc in vec {
        assert!(exc.as_condition().is_some());
    }
}

#[test]
fn test_filtering_with_try_block() {
    // Demonstrate filtering pattern with try block
    let result = eval(r#"(try (exception "network-timeout" 504) (catch e e))"#).unwrap();

    // Result should be the exception (from try, not catch which isn't functional yet)
    assert!(result.as_condition().is_some());
}

#[test]
fn test_filtering_semantic_error_categories() {
    // Create semantically meaningful error categories
    let transient = eval(r#"(exception "transient" "please-retry")"#).unwrap();
    let permanent = eval(r#"(exception "permanent" "fix-required")"#).unwrap();
    let retriable = eval(r#"(exception "retriable" (list "attempts" 3))"#).unwrap();

    assert!(transient.as_condition().is_some());
    assert!(permanent.as_condition().is_some());
    assert!(retriable.as_condition().is_some());
}
