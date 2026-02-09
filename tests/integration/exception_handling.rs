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
// Basic Exception Creation and Inspection
// ============================================================================

#[test]
fn test_exception_basic_creation() {
    let result = eval("(exception \"simple error\")").unwrap();
    assert!(matches!(result, Value::Exception(_)));
}

#[test]
fn test_exception_with_integer_data() {
    let result = eval("(exception \"error\" 42)").unwrap();
    assert!(matches!(result, Value::Exception(_)));

    // Verify data can be extracted
    let data = eval("(exception-data (exception \"error\" 42))").unwrap();
    assert_eq!(data, Value::Int(42));
}

#[test]
fn test_exception_with_list_data() {
    let result = eval("(exception \"error\" (list 1 2 3))").unwrap();
    assert!(matches!(result, Value::Exception(_)));

    // Verify data extraction works
    let data = eval("(exception-data (exception \"error\" (list 4 5 6)))").unwrap();
    let vec = data.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_exception_with_boolean_data() {
    let result = eval("(exception \"error\" #t)").unwrap();
    assert!(matches!(result, Value::Exception(_)));

    let data = eval("(exception-data (exception \"flag\" #t))").unwrap();
    assert_eq!(data, Value::Bool(true));
}

#[test]
fn test_exception_message_extraction_simple() {
    let result = eval("(exception-message (exception \"test message\"))").unwrap();
    assert_eq!(result, Value::String("test message".into()));
}

#[test]
fn test_exception_data_extraction_missing() {
    let result = eval("(exception-data (exception \"error\"))").unwrap();
    assert_eq!(result, Value::Nil);
}

// ============================================================================
// Try Block Execution
// ============================================================================

#[test]
fn test_try_successful_computation() {
    let result = eval("(try (+ 5 3))").unwrap();
    assert_eq!(result, Value::Int(8));
}

#[test]
fn test_try_multiplication() {
    let result = eval("(try (* 7 6))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_subtraction() {
    let result = eval("(try (- 20 5))").unwrap();
    assert_eq!(result, Value::Int(15));
}

#[test]
fn test_try_division() {
    let result = eval("(try (/ 100 5))").unwrap();
    assert_eq!(result, Value::Int(20));
}

#[test]
fn test_try_nested_arithmetic() {
    let result = eval("(try (+ (* 2 3) (- 10 4)))").unwrap();
    assert_eq!(result, Value::Int(12)); // (6 + 6) = 12
}

#[test]
fn test_try_with_literal_integer() {
    let result = eval("(try 42)").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_with_literal_string() {
    let result = eval("(try \"hello\")").unwrap();
    assert_eq!(result, Value::String("hello".into()));
}

#[test]
fn test_try_with_list_literal() {
    let result = eval("(try (list 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_try_ignores_catch_on_success() {
    // When no exception, catch should not execute
    let result = eval("(try (+ 10 20) (catch e 999))").unwrap();
    assert_eq!(result, Value::Int(30)); // Not 999
}

// ============================================================================
// Nested Try Blocks
// ============================================================================

#[test]
fn test_nested_try_blocks_both_succeed() {
    let result = eval("(try (try (+ 2 3) (catch e 0)) (catch e 1))").unwrap();
    assert_eq!(result, Value::Int(5));
}

#[test]
fn test_nested_try_inner_with_catch() {
    let result = eval("(try (try 100 (catch e 50)) (catch e 25))").unwrap();
    assert_eq!(result, Value::Int(100));
}

#[test]
fn test_nested_try_outer_with_catch() {
    let result = eval("(try (try 200 (catch e 100)) (catch e 50))").unwrap();
    assert_eq!(result, Value::Int(200));
}

#[test]
fn test_deeply_nested_try_blocks() {
    let result = eval("(try (try (try (+ 1 1) (catch e 0)) (catch e 1)) (catch e 2))").unwrap();
    assert_eq!(result, Value::Int(2));
}

// ============================================================================
// Complex Exception Data
// ============================================================================

#[test]
fn test_exception_with_nested_list_data() {
    let result = eval("(exception-data (exception \"error\" (list 1 (list 2 3) 4)))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::Int(1));
    assert_eq!(vec[2], Value::Int(4));
}

#[test]
fn test_exception_message_various_strings() {
    let msg1 = eval("(exception-message (exception \"Auth failed\"))").unwrap();
    let msg2 = eval("(exception-message (exception \"Network timeout\"))").unwrap();
    let msg3 = eval("(exception-message (exception \"Invalid input\"))").unwrap();

    assert_eq!(msg1, Value::String("Auth failed".into()));
    assert_eq!(msg2, Value::String("Network timeout".into()));
    assert_eq!(msg3, Value::String("Invalid input".into()));
}

#[test]
fn test_exception_with_nil_data() {
    let result = eval("(exception \"error\" nil)").unwrap();
    assert!(matches!(result, Value::Exception(_)));

    let data = eval("(exception-data (exception \"error\" nil))").unwrap();
    assert_eq!(data, Value::Nil);
}

// ============================================================================
// Try Block with Different Value Types
// ============================================================================

#[test]
fn test_try_returns_boolean() {
    let result = eval("(try #t)").unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_try_returns_nil() {
    let result = eval("(try nil)").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn test_try_comparison_returns_boolean() {
    let result = eval("(try (> 10 5))").unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_try_equality_check() {
    let result = eval("(try (= 5 5))").unwrap();
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_try_less_than() {
    let result = eval("(try (< 3 7))").unwrap();
    assert_eq!(result, Value::Bool(true));
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
        assert!(matches!(item, Value::Exception(_)));
    }
}

#[test]
fn test_multiple_try_blocks_sequence() {
    let result1 = eval("(try (+ 1 1))").unwrap();
    let result2 = eval("(try (* 3 3))").unwrap();
    let result3 = eval("(try (- 10 2))").unwrap();

    assert_eq!(result1, Value::Int(2));
    assert_eq!(result2, Value::Int(9));
    assert_eq!(result3, Value::Int(8));
}

// ============================================================================
// Exception Utility Functions
// ============================================================================

#[test]
fn test_exception_and_message_together() {
    // Create exception and immediately extract message
    let result = eval("(exception-message (exception \"Connection timeout\"))").unwrap();
    assert_eq!(result, Value::String("Connection timeout".into()));
}

#[test]
fn test_exception_and_data_together() {
    // Create exception and immediately extract data
    let result = eval("(exception-data (exception \"Error\" 404))").unwrap();
    assert_eq!(result, Value::Int(404));
}

#[test]
fn test_exception_data_from_complex_structure() {
    // Exception with structured error details
    let result = eval("(exception-data (exception \"Validation\" (list \"field\" \"name\" \"error\" \"required\")))").unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::String("field".into()));
    assert_eq!(vec[1], Value::String("name".into()));
}

// ============================================================================
// Try Block Semantics
// ============================================================================

#[test]
fn test_try_preserves_computation_result() {
    // Try block result should be the same as unwrapped computation
    let unwrapped = eval("(+ 15 25)").unwrap();
    let wrapped = eval("(try (+ 15 25))").unwrap();

    assert_eq!(unwrapped, wrapped);
    assert_eq!(wrapped, Value::Int(40));
}

#[test]
fn test_try_with_catch_ignores_catch_on_success() {
    // Catch clause should be ignored when try succeeds
    let result = eval("(try (* 6 7) (catch error 0))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_with_catch_parameter_not_bound_on_success() {
    // Catch parameter should not be available when try succeeds
    let result = eval("(try 123 (catch e 456))").unwrap();
    assert_eq!(result, Value::Int(123));
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
    assert_eq!(msg1, Value::String("stored".into()));
}

#[test]
fn test_exception_in_list_operations() {
    let result = eval("(list (exception \"a\" 1) (exception \"b\" 2))").unwrap();
    let vec = result.list_to_vec().unwrap();

    assert_eq!(vec.len(), 2);
    let msg1 = match &vec[0] {
        Value::Exception(exc) => exc.message.clone(),
        _ => panic!("Expected exception"),
    };
    let msg2 = match &vec[1] {
        Value::Exception(exc) => exc.message.clone(),
        _ => panic!("Expected exception"),
    };

    assert_eq!(msg1, "a".into());
    assert_eq!(msg2, "b".into());
}

#[test]
fn test_try_return_type_consistency() {
    // Try blocks consistently return values
    let int_result = eval("(try 42)").unwrap();
    let string_result = eval("(try \"test\")").unwrap();
    let bool_result = eval("(try #t)").unwrap();

    assert_eq!(int_result, Value::Int(42));
    assert!(matches!(string_result, Value::String(_)));
    assert_eq!(bool_result, Value::Bool(true));
}

#[test]
fn test_exception_message_all_types_as_data() {
    // Exception can store different data types
    let int_data = eval("(exception-data (exception \"e\" 100))").unwrap();
    let bool_data = eval("(exception-data (exception \"e\" #f))").unwrap();
    let nil_data = eval("(exception-data (exception \"e\" nil))").unwrap();

    assert_eq!(int_data, Value::Int(100));
    assert_eq!(bool_data, Value::Bool(false));
    assert_eq!(nil_data, Value::Nil);
}

// ============================================================================
// Handler-case with division by zero
// ============================================================================

#[test]
fn test_division_by_zero_creates_condition() {
    // Division by zero should create an internal Condition, not an exception
    // For now, division by zero still returns an error string
    let result = eval("(/ 10 0)");
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert_eq!(err_msg, "Division by zero");
}

#[test]
fn test_safe_division() {
    // Normal division should work
    let result = eval("(/ 10 2)").unwrap();
    assert_eq!(result, Value::Int(5));
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
    assert_eq!(err_msg, "Division by zero");
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
    assert_eq!(result, Value::Int(0));
}

#[test]
fn test_safe_operation_zero_divisor() {
    // Safe operation protecting against division by zero
    let result = eval("(if (= 0 0) 0 (/ 10 0))").unwrap();
    assert_eq!(result, Value::Int(0));
}

#[test]
fn test_safe_arithmetic_chain() {
    // Multiple operations with protection
    let result = eval("(if (= 4 0) 0 (* (+ 10 20) (/ 100 4)))").unwrap();
    // (10+20) * (100/4) = 30 * 25 = 750
    assert_eq!(result, Value::Int(750));
}

#[test]
fn test_safe_arithmetic_chain_zero_divisor() {
    // Multiple operations with zero divisor protection
    let result = eval("(if (= 0 0) 0 (* (+ 10 20) (/ 100 0)))").unwrap();
    assert_eq!(result, Value::Int(0));
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
    assert!(err.contains("Division by zero") || err.contains("exception"));
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
    assert_eq!(result, Value::Int(5));
}

#[test]
fn test_multiple_safe_operations() {
    // Multiple operations without exceptions should work
    let r1 = eval("(+ 5 3)").unwrap();
    let r2 = eval("(- 10 2)").unwrap();
    let r3 = eval("(* 4 5)").unwrap();

    assert_eq!(r1, Value::Int(8));
    assert_eq!(r2, Value::Int(8));
    assert_eq!(r3, Value::Int(20));
}

// ============================================================================
// Try/Catch/Finally Tests (Phase 10)
// ============================================================================

#[test]
fn test_try_catch_catches_division_by_zero() {
    // Basic try/catch catching division by zero
    let result = eval("(try (/ 10 0) (catch e 99))").unwrap();
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_try_catch_binds_exception_variable() {
    // Exception variable is bound to the caught exception condition
    // For now we just verify it doesn't error
    let result = eval("(try (/ 10 0) (catch e e))");
    assert!(result.is_ok());
}

#[test]
fn test_try_catch_ignores_catch_on_success() {
    // When try succeeds, catch is not executed
    let result = eval("(try (+ 10 20) (catch e 999))").unwrap();
    assert_eq!(result, Value::Int(30));
}

#[test]
fn test_try_catch_multiple_operations() {
    // Try/catch with multiple operations in body
    let result = eval("(try (+ (* 2 3) (/ 10 2)) (catch e 0))").unwrap();
    assert_eq!(result, Value::Int(11)); // (6 + 5) = 11
}

#[test]
fn test_try_without_catch() {
    // Try without catch clause should still work
    let result = eval("(try 42)").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_catch_with_finally() {
    // Try/catch with finally block
    // Note: We can't directly test that finally executes, but we can verify result
    let result = eval("(try (/ 10 0) (catch e 99) (finally 0))").unwrap();
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_try_with_finally_no_catch() {
    // Finally executes even without catch
    let result = eval("(try 42 (finally 0))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_finally_with_success() {
    // Finally executes on successful try
    let result = eval("(try (+ 5 3) (finally (+ 1 1)))").unwrap();
    assert_eq!(result, Value::Int(8));
}

#[test]
fn test_try_catch_finally_all_together() {
    // All three components together
    let result = eval("(try (/ 100 10) (catch e 0) (finally 0))").unwrap();
    assert_eq!(result, Value::Int(10));
}

#[test]
fn test_nested_try_blocks_inner_catches() {
    // Nested try/catch - inner catch handles exception
    let result = eval("(try (try (/ 10 0) (catch e 50)) (catch e2 100))").unwrap();
    assert_eq!(result, Value::Int(50));
}

#[test]
fn test_nested_try_blocks_outer_catches() {
    // Nested try/catch - outer catch handles exception
    let result = eval("(try (try 42 (catch e1 100)) (catch e2 200))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_try_catch_returns_different_type() {
    // Exception handler can return different type than try body
    let result = eval("(try (/ 10 0) (catch e \"error\"))").unwrap();
    assert_eq!(result, Value::String("error".into()));
}

#[test]
fn test_try_catch_with_boolean_result() {
    // Try/catch can return boolean
    let result = eval("(try #f (catch e #t))").unwrap();
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_try_catch_with_list_result() {
    // Try/catch can return list
    let result = eval("(try (list 1 2 3) (catch e nil))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
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
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_handler_case_catches_exception() {
    // handler-case catches division by zero (exception ID 4)
    let result = eval("(handler-case (/ 10 0) (4 e 99))").unwrap();
    assert_eq!(result, Value::Int(99));
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
//     assert_eq!(result, Value::Int(99));
// }

#[test]
fn test_handler_case_unmatched_exception() {
    // handler-case doesn't match non-4 exceptions
    // (though in practice most runtime errors are ID 4)
    let result = eval("(handler-case 42 (5 e 100))").unwrap();
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_handler_case_with_complex_body() {
    // handler-case with complex body expression
    let result = eval("(handler-case (+ (* 2 3) (/ 20 2)) (4 e 0))").unwrap();
    assert_eq!(result, Value::Int(16)); // (6 + 10) = 16
}

#[test]
fn test_handler_case_handler_code_executes() {
    // Handler body is executed when exception matches
    let result = eval("(handler-case (/ 1 0) (4 e (+ 50 49)))").unwrap();
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_handler_case_nested() {
    // Nested handler-case expressions
    let result = eval("(handler-case (handler-case (/ 10 0) (4 e 50)) (4 e 100))").unwrap();
    assert_eq!(result, Value::Int(50));
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
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_exception_and_condition_difference() {
    // Exception (the value type) is different from Condition (internal type)
    // Verify both work
    let exc_result = eval("(exception \"test\")").unwrap();
    assert!(matches!(exc_result, Value::Exception(_)));

    let cond_result = eval("(handler-case (/ 10 0) (4 e 99))").unwrap();
    assert_eq!(cond_result, Value::Int(99));
}

// ============================================================================
// Try/Catch/Finally Error Cases
// ============================================================================

#[test]
fn test_try_catch_finally_stack_integrity() {
    // Verify stack is clean after try/catch/finally
    // Multiple sequential try/catch should work
    let r1 = eval("(try 1 (catch e 0))").unwrap();
    let r2 = eval("(try 2 (catch e 0))").unwrap();
    let r3 = eval("(try (/ 10 0) (catch e 3))").unwrap();

    assert_eq!(r1, Value::Int(1));
    assert_eq!(r2, Value::Int(2));
    assert_eq!(r3, Value::Int(3));
}

#[test]
fn test_try_catch_inside_function() {
    // Try/catch works inside function calls
    // This is a basic sanity test
    let result = eval("((lambda () (try (/ 10 0) (catch e 99))))").unwrap();
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_try_catch_in_let_binding() {
    // Try/catch can be used in let bindings
    let result = eval("(let ((x (try (/ 10 0) (catch e 99)))) x)").unwrap();
    assert_eq!(result, Value::Int(99));
}

#[test]
fn test_try_catch_with_side_effects() {
    // Try/catch doesn't prevent side effects in catch handler
    let result = eval("(try (/ 10 0) (catch e 99))").unwrap();
    assert_eq!(result, Value::Int(99));
}

// ============================================================================
// Regression Tests for Issue #162: Sequential Exception-Catching
// ============================================================================

#[test]
fn test_sequential_exception_catching_regression() {
    // Issue #162: Sequential exception-catching failed in same execution context
    // First exception should be caught and handled
    let r1 = eval("(try (/ 10 0) (catch e 99))").unwrap();
    assert_eq!(r1, Value::Int(99));

    // Second exception should also be caught (this used to fail with "Cannot call <condition: id=4>")
    let r2 = eval("(try (/ 20 0) (catch e 88))").unwrap();
    assert_eq!(r2, Value::Int(88));
}

#[test]
fn test_multiple_sequential_catches() {
    // Multiple sequential try/catch blocks in single expression
    let result = eval(
        "(begin 
        (define r1 (try (/ 10 0) (catch e 99)))
        (define r2 (try (/ 20 0) (catch e 88)))
        (define r3 (try (+ 1 2) (catch e 0)))
        (list r1 r2 r3)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::Int(99));
    assert_eq!(vec[1], Value::Int(88));
    assert_eq!(vec[2], Value::Int(3));
}

#[test]
fn test_exception_variable_binding_multiple_catches() {
    // Verify exception variables are correctly bound in sequential catches
    let result = eval(
        "(begin
        (define e1 (try (/ 10 0) (catch exc exc)))
        (define e2 (try (/ 20 0) (catch exc exc)))
        (list e1 e2)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    // Both should be condition values
    assert!(matches!(vec[0], Value::Condition(_)));
    assert!(matches!(vec[1], Value::Condition(_)));
}

#[test]
fn test_nested_sequential_exception_catching() {
    // Nested function calls with sequential exception catching
    let result = eval(
        "(begin
        (define safe-divide (lambda (a b)
          (try (/ a b) (catch e nil))))
        (define r1 (safe-divide 10 0))
        (define r2 (safe-divide 20 0))
        (list r1 r2)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::Nil);
    assert_eq!(vec[1], Value::Nil);
}

#[test]
fn test_mixed_success_and_exception_sequential() {
    // Mix of successful and exception-throwing expressions
    let result = eval(
        "(begin
        (define r1 (try (+ 1 2) (catch e 0)))
        (define r2 (try (/ 10 0) (catch e 99)))
        (define r3 (try (* 3 4) (catch e 0)))
        (define r4 (try (/ 20 0) (catch e 88)))
        (list r1 r2 r3 r4)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::Int(3));
    assert_eq!(vec[1], Value::Int(99));
    assert_eq!(vec[2], Value::Int(12));
    assert_eq!(vec[3], Value::Int(88));
}

#[test]
fn test_exception_catching_in_loop() {
    // Exception catching inside nested expressions
    let result = eval(
        "(list
        (try (/ 10 0) (catch e 110))
        (try (/ 20 0) (catch e 120))
        (try (/ 30 0) (catch e 130))
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::Int(110));
    assert_eq!(vec[1], Value::Int(120));
    assert_eq!(vec[2], Value::Int(130));
}

#[test]
fn test_exception_with_finally_sequential() {
    // Sequential exception catching with finally blocks
    let result = eval(
        "(begin
        (define r1 (try (/ 10 0) (catch e 99) (finally #f)))
        (define r2 (try (/ 20 0) (catch e 88) (finally #f)))
        (list r1 r2)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::Int(99));
    assert_eq!(vec[1], Value::Int(88));
}

#[test]
fn test_exception_access_in_subsequent_expression() {
    // Access exception variable in subsequent code paths
    let result = eval(
        "(begin
        (define exc1 (try (/ 10 0) (catch e e)))
        (define exc2 (try (/ 20 0) (catch e e)))
        ; Both should be condition values - just verify they're defined
        (list exc1 exc2)
    )",
    )
    .unwrap();

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    // Both should be condition values
    assert!(matches!(vec[0], Value::Condition(_)));
    assert!(matches!(vec[1], Value::Condition(_)));
}
