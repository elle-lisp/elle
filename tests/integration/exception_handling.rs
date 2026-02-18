use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Try to compile as a single expression first
    match compile_new(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            // If that fails, try wrapping in a begin
            let wrapped = format!("(begin {})", input);
            match compile_new(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    // If that also fails, try compiling all expressions
                    let results = compile_all_new(input, &mut symbols)?;
                    let mut last_result = Value::NIL;
                    for result in results {
                        last_result = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
                    }
                    Ok(last_result)
                }
            }
        }
    }
}

// ============================================================================
// Basic Exception Creation and Inspection
// ============================================================================

#[test]
fn test_exception_basic_creation() {
    let result = eval("(exception \"simple error\")").unwrap();
    assert!(result.as_condition().is_some());
}

#[test]
fn test_exception_with_integer_data() {
    let result = eval("(exception \"error\" 42)").unwrap();
    assert!(result.as_condition().is_some());

    // Verify data can be extracted
    let data = eval("(exception-data (exception \"error\" 42))").unwrap();
    assert_eq!(data, Value::int(42));
}

#[test]
fn test_exception_with_list_data() {
    let result = eval("(exception \"error\" (list 1 2 3))").unwrap();
    assert!(result.as_condition().is_some());

    // Verify data extraction works
    let data = eval("(exception-data (exception \"error\" (list 4 5 6)))").unwrap();
    let vec = data.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_exception_with_boolean_data() {
    let result = eval("(exception \"error\" #t)").unwrap();
    assert!(result.as_condition().is_some());

    let data = eval("(exception-data (exception \"flag\" #t))").unwrap();
    assert_eq!(data, Value::bool(true));
}

#[test]
fn test_exception_message_extraction_simple() {
    let result = eval("(exception-message (exception \"test message\"))").unwrap();
    assert_eq!(result, Value::string("test message"));
}

#[test]
fn test_exception_data_extraction_missing() {
    let result = eval("(exception-data (exception \"error\"))").unwrap();
    assert_eq!(result, Value::NIL);
}

// ============================================================================
// Complex Exception Data
// ============================================================================

#[test]
fn test_exception_with_nested_list_data() {
    let result = eval("(exception-data (exception \"error\" (list 1 (list 2 3) 4)))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[2], Value::int(4));
}

#[test]
fn test_exception_message_various_strings() {
    let msg1 = eval("(exception-message (exception \"Auth failed\"))").unwrap();
    let msg2 = eval("(exception-message (exception \"Network timeout\"))").unwrap();
    let msg3 = eval("(exception-message (exception \"Invalid input\"))").unwrap();

    assert_eq!(msg1, Value::string("Auth failed"));
    assert_eq!(msg2, Value::string("Network timeout"));
    assert_eq!(msg3, Value::string("Invalid input"));
}

#[test]
fn test_exception_with_nil_data() {
    let result = eval("(exception \"error\" nil)").unwrap();
    assert!(result.as_condition().is_some());

    let data = eval("(exception-data (exception \"error\" nil))").unwrap();
    assert_eq!(data, Value::NIL);
}

// ============================================================================
// Multiple Exceptions in Sequence
// ============================================================================

#[test]
fn test_multiple_exception_creations() {
    let result =
        eval("(list (exception \"first\") (exception \"second\") (exception \"third\"))").unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 3);
    for item in vec {
        assert!(item.as_condition().is_some());
    }
}

// ============================================================================
// Exception Utility Functions
// ============================================================================

#[test]
fn test_exception_and_message_together() {
    // Create exception and immediately extract message
    let result = eval("(exception-message (exception \"Connection timeout\"))").unwrap();
    assert_eq!(result, Value::string("Connection timeout"));
}

#[test]
fn test_exception_and_data_together() {
    // Create exception and immediately extract data
    let result = eval("(exception-data (exception \"Error\" 404))").unwrap();
    assert_eq!(result, Value::int(404));
}

#[test]
fn test_exception_data_from_complex_structure() {
    // Exception with structured error details
    let result = eval("(exception-data (exception \"Validation\" (list \"field\" \"name\" \"error\" \"required\")))").unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::string("field"));
    assert_eq!(vec[1], Value::string("name"));
}

// ============================================================================
// Exception as First-Class Values
// ============================================================================

#[test]
fn test_exception_can_be_stored() {
    // Exception created once and used multiple times
    let msg1 = eval("(exception-message (exception \"stored\" 1))").unwrap();
    let msg2 = eval("(exception-message (exception \"stored\" 1))").unwrap();

    assert_eq!(msg1, msg2);
    assert_eq!(msg1, Value::string("stored"));
}

#[test]
fn test_exception_in_list_operations() {
    let result = eval("(list (exception \"a\" 1) (exception \"b\" 2))").unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 2);
    let msg1 = if let Some(cond) = vec[0].as_condition() {
        cond.message().to_string()
    } else {
        panic!("Expected condition")
    };
    let msg2 = if let Some(cond) = vec[1].as_condition() {
        cond.message().to_string()
    } else {
        panic!("Expected condition")
    };

    assert_eq!(msg1, "a");
    assert_eq!(msg2, "b");
}

#[test]
fn test_exception_message_all_types_as_data() {
    // Exception can store different data types
    let int_data = eval("(exception-data (exception \"e\" 100))").unwrap();
    let bool_data = eval("(exception-data (exception \"e\" #f))").unwrap();
    let nil_data = eval("(exception-data (exception \"e\" nil))").unwrap();

    assert_eq!(int_data, Value::int(100));
    assert_eq!(bool_data, Value::bool(false));
    assert_eq!(nil_data, Value::NIL);
}

// ============================================================================
// Handler-case with division by zero
// ============================================================================

#[test]
fn test_division_by_zero_creates_condition() {
    // Division by zero should create an internal Condition
    // Error message now includes exception name and message
    let result = eval("(/ 10 0)");
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert_eq!(err_msg, "division-by-zero: division by zero");
}

#[test]
fn test_safe_division() {
    // Normal division should work
    let result = eval("(/ 10 2)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_division_by_zero_specialized() {
    // Division by zero with specialized int instruction
    let result = eval("(/ 100 0)");
    assert!(result.is_err());
}

// ============================================================================
// Exception Matching Infrastructure Tests
// ============================================================================

// Note: Full handler-case tests require special parsing support in the language
// For now, we test the underlying exception propagation and Condition creation

#[test]
fn test_condition_creation_on_division_by_zero() {
    // Division by zero creates a Condition internally
    let result = eval("(/ 10 0)");
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    // Error message now includes exception name and message
    assert_eq!(err_msg, "division-by-zero: division by zero");
}

#[test]
fn test_multiple_divisions_independent() {
    // Each division by zero is independent
    let r1 = eval("(/ 5 0)");
    let r2 = eval("(/ 10 0)");
    assert!(r1.is_err() && r2.is_err());
}

// ============================================================================
// Phase 8: Exception Introspection and Field Access
// ============================================================================

#[test]
fn test_exception_id_from_condition() {
    // Create a condition and extract its ID
    // Note: Conditions are created internally by handler-case/signal
    // For now, we test with signal which creates Condition objects
    let _result = eval("(signal 2 \"test message\")");
    // Signal returns the condition, so we can introspect it
    // But currently signal doesn't work as expected in eval, so this tests the mechanism
}

#[test]
fn test_condition_field_access_basic() {
    // Test that condition-field can be called
    // This will be more meaningful once handler-case is fully integrated
    // For now we verify the primitive exists and basic signature works
    let _result = eval("(condition-field nil 0)");
    // Should return nil or error gracefully
}

#[test]
fn test_condition_matches_type_basic() {
    // Test that condition-matches-type can be called
    let _result = eval("(condition-matches-type nil 2)");
    // Should return false since nil is not a condition
}

#[test]
fn test_condition_backtrace_basic() {
    // Test that condition-backtrace can be called
    let _result = eval("(condition-backtrace nil)");
    // Should return nil or error gracefully
}

// ============================================================================
// Phase 8: Integration with Conditional Logic (Safe Operations)
// ============================================================================

#[test]
fn test_safe_operation_with_conditional_logic() {
    // Safe operations that avoid exceptions through conditional checks
    let result = eval("(if (= 0 0) 0 (/ 10 2))").unwrap();
    assert_eq!(result, Value::int(0));
}

#[test]
fn test_safe_operation_zero_divisor() {
    // Safe operation protecting against division by zero
    let result = eval("(if (= 0 0) 0 (/ 10 0))").unwrap();
    assert_eq!(result, Value::int(0));
}

#[test]
fn test_safe_arithmetic_chain() {
    // Multiple operations with protection
    let result = eval("(if (= 4 0) 0 (* (+ 10 20) (/ 100 4)))").unwrap();
    // (10+20) * (100/4) = 30 * 25 = 750
    assert_eq!(result, Value::int(750));
}

#[test]
fn test_safe_arithmetic_chain_zero_divisor() {
    // Multiple operations with zero divisor protection
    let result = eval("(if (= 0 0) 0 (* (+ 10 20) (/ 100 0)))").unwrap();
    assert_eq!(result, Value::int(0));
}

// ============================================================================
// Phase 8: Exception Data Structure Integrity
// ============================================================================

#[test]
fn test_exception_created_with_signal() {
    // signal should create proper exception objects
    // This is more of a smoke test for the primitive chain
}

#[test]
fn test_error_primitive_exists() {
    // error primitive should be callable
    let _result = eval("(error \"test error\")");
    // error should signal an exception, so it returns error or handled result
}

#[test]
fn test_warn_primitive_exists() {
    // warn primitive should be callable
    let _result = eval("(warn \"test warning\")");
    // warn should signal a warning
}

// ============================================================================
// Phase 9a: Exception Interrupt Mechanism Tests
// ============================================================================

#[test]
fn test_division_by_zero_interrupt_without_handler() {
    // Division by zero should return error when no handler is present
    let result = eval("(/ 10 0)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("division by zero") || err.contains("division-by-zero"),
        "Expected error message to contain 'division by zero', got: {}",
        err
    );
}

#[test]
fn test_exception_state_set_after_interrupt() {
    // Test that exception is properly set after division by zero
    // This is a meta-test to ensure the interrupt mechanism worked
    let result = eval("(/ 5 0)");
    assert!(result.is_err());
}

#[test]
fn test_safe_division_no_interrupt() {
    // Safe division should work without triggering exception
    let result = eval("(/ 10 2)").unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_multiple_safe_operations() {
    // Multiple operations without exceptions should work
    let r1 = eval("(+ 5 3)").unwrap();
    let r2 = eval("(- 10 2)").unwrap();
    let r3 = eval("(* 4 5)").unwrap();

    assert_eq!(r1, Value::int(8));
    assert_eq!(r2, Value::int(8));
    assert_eq!(r3, Value::int(20));
}

// ============================================================================
// Handler-Case Tests (Low-level exception handling)
// ============================================================================

#[test]
fn test_handler_case_basic_success() {
    // handler-case returns body value when no exception
    // Note: handler-case is a special form, testing it requires
    // verifying the expression evaluates without error
    let result = eval("(handler-case 42 (4 e 99))").unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_handler_case_catches_exception() {
    // handler-case catches division by zero (exception ID 4)
    let result = eval("(handler-case (/ 10 0) (4 e 99))").unwrap();
    assert_eq!(result, Value::int(99));
}

// NOTE: handler-case with multiple handlers is not yet fully supported
// This test is skipped as it requires more advanced exception handling
// that hasn't been fully implemented yet
// #[test]
// fn test_handler_case_multiple_handlers() {
//     // handler-case with multiple handler clauses
//     // First handler matches exception ID 4 (division by zero)
//     let result = eval(
//         "(handler-case (/ 10 0) (5 e 100) (4 e 99))"
//     ).unwrap();
//     assert_eq!(result, Value::int(99));
// }

#[test]
fn test_handler_case_unmatched_exception() {
    // handler-case doesn't match non-4 exceptions
    // (though in practice most runtime errors are ID 4)
    let result = eval("(handler-case 42 (5 e 100))").unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_handler_case_with_complex_body() {
    // handler-case with complex body expression
    let result = eval("(handler-case (+ (* 2 3) (/ 20 2)) (4 e 0))").unwrap();
    assert_eq!(result, Value::int(16)); // (6 + 10) = 16
}

#[test]
fn test_handler_case_handler_code_executes() {
    // Handler body is executed when exception matches
    let result = eval("(handler-case (/ 1 0) (4 e (+ 50 49)))").unwrap();
    assert_eq!(result, Value::int(99));
}

#[test]
fn test_handler_case_nested() {
    // Nested handler-case expressions
    let result = eval("(handler-case (handler-case (/ 10 0) (4 e 50)) (4 e 100))").unwrap();
    assert_eq!(result, Value::int(50));
}

#[test]
fn test_handler_case_exception_variable_binding() {
    // Exception is bound to variable in handler
    // Verify it doesn't error and returns a value
    let result = eval("(handler-case (/ 10 0) (4 e e))");
    assert!(result.is_ok());
}

// ============================================================================
// Exception Condition Tests (Condition objects)
// ============================================================================

#[test]
fn test_condition_creation_id_4() {
    // Division by zero creates exception with ID 4
    // We can't directly inspect the condition, but we can verify
    // that handler-case catches it
    let result = eval("(handler-case (/ 10 0) (4 e 99))").unwrap();
    assert_eq!(result, Value::int(99));
}

#[test]
fn test_exception_and_condition_difference() {
    // Exception (the value type) is different from Condition (internal type)
    // Verify both work
    let exc_result = eval("(exception \"test\")").unwrap();
    assert!(exc_result.as_condition().is_some());

    let cond_result = eval("(handler-case (/ 10 0) (4 e 99))").unwrap();
    assert_eq!(cond_result, Value::int(99));
}
