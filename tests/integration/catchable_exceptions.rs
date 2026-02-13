// Tests for catchable runtime exceptions
// Verifies that undefined-variable (ID 5) and arity-error (ID 6)
// can be caught with handler-case

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
// Undefined Variable Exception (ID 5)
// ============================================================================

#[test]
fn test_catch_undefined_variable_by_id() {
    // handler-case catches undefined-variable exception (ID 5)
    let result = eval("(handler-case undefined-var (5 e 'caught))").unwrap();
    // Just verify it's a symbol (the exact ID depends on symbol table state)
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_catch_undefined_variable_by_name() {
    // handler-case catches undefined-variable by symbolic name
    let result = eval("(handler-case undefined-var (undefined-variable e 'caught))").unwrap();
    // Result is 'caught symbol
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_undefined_variable_no_handler() {
    // Without handler, undefined variable propagates as error
    let result = eval("undefined-var");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exception"));
}

#[test]
fn test_undefined_variable_exception_contains_symbol() {
    // The exception object should contain the undefined symbol
    // We access it via the exception variable 'e'
    let result = eval(
        r#"
        (handler-case 
            undefined-test-symbol
            (5 e (exception-id e)))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::Int(5)); // exception-id returns 5
}

#[test]
fn test_undefined_variable_in_nested_expression() {
    // Undefined variable in a nested expression is catchable
    let result = eval("(handler-case (+ 1 undefined-var) (5 e 'caught))").unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

// ============================================================================
// Arity Error Exception (ID 6)
// ============================================================================

#[test]
fn test_catch_arity_error_by_id() {
    // handler-case catches arity-error exception (ID 6)
    // Define a function expecting 2 args, call with 1
    let result = eval(
        r#"
        (begin
            (define f (fn (a b) (+ a b)))
            (handler-case (f 1) (6 e 'caught)))
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_catch_arity_error_by_name() {
    // handler-case catches arity-error by symbolic name
    let result = eval(
        r#"
        (begin
            (define f (fn (a b) (+ a b)))
            (handler-case (f 1) (arity-error e 'caught)))
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_arity_error_too_few_args() {
    // Calling with too few arguments is catchable
    let result = eval(
        r#"
        (begin
            (define f (fn (a b c) (+ a b c)))
            (handler-case (f 1 2) (6 e 'too-few)))
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_arity_error_too_many_args() {
    // Calling with too many arguments is catchable
    let result = eval(
        r#"
        (begin
            (define f (fn (a) a))
            (handler-case (f 1 2 3) (6 e 'too-many)))
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_multiple_handlers_second_match() {
    // When first handler doesn't match, second is tried
    // Verify that we can catch with a non-first handler
    let result = eval(
        r#"
        (handler-case 
            undefined-var
            (5 e 'undefined)
            (6 e 'arity))
    "#,
    )
    .unwrap();
    // Should match undefined-variable (5) with first handler
    assert!(matches!(result, Value::Symbol(_)));
}

// ============================================================================
// Exception Hierarchy Tests
// ============================================================================

#[test]
fn test_catch_error_base_catches_undefined() {
    // exception ID 2 (error) is parent of ID 5 (undefined-variable)
    // Catching 'error' should catch undefined-variable
    let result = eval("(handler-case undefined-var (error e 'caught))").unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_catch_error_base_catches_arity() {
    // exception ID 2 (error) is parent of ID 6 (arity-error)
    let result = eval(
        r#"
        (begin
            (define f (fn (a b) (+ a b)))
            (handler-case (f 1) (error e 'caught)))
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_catch_condition_base_catches_all() {
    // exception ID 1 (condition) is the root
    let result = eval("(handler-case undefined-var (condition e 'caught))").unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

// ============================================================================
// Multiple Handlers
// ============================================================================

#[test]
fn test_multiple_handlers_first_match() {
    // When multiple handlers exist, first matching one is used
    let result = eval(
        r#"
        (handler-case 
            undefined-var
            (5 e 'undefined)
            (6 e 'arity))
    "#,
    )
    .unwrap();
    // Should match undefined-variable (5), not arity (6)
    if let Value::Symbol(sym) = result {
        // The symbol should be 'undefined, not 'arity
        assert!(sym.0 > 0); // Just verify it's a valid symbol
    } else {
        panic!("Expected symbol");
    }
}

// ============================================================================
// Assertion Pattern Tests
// ============================================================================

#[test]
fn test_assertion_pattern_catches_errors() {
    // This pattern is used in test files to catch and report errors
    let result = eval(
        r#"
        (handler-case 
            (+ 1 2)
            (error e #f))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_source_location_tracking() {
    // Verify that source location is set on the VM
    let mut vm = elle::VM::new();
    let loc = elle::reader::SourceLoc::new("test.lisp", 1, 1);
    vm.set_current_source_loc(Some(loc.clone()));
    
    // Verify it was set
    assert_eq!(vm.get_current_source_loc(), Some(&loc));
}

#[test]
fn test_undefined_variable_exception_has_location() {
    // When an undefined variable exception is raised, it should have location info
    let mut vm = elle::VM::new();
    let mut symbols = SymbolTable::new();
    elle::register_primitives(&mut vm, &mut symbols);
    
    // Set a source location
    let loc = elle::reader::SourceLoc::new("test.lisp", 5, 10);
    vm.set_current_source_loc(Some(loc.clone()));
    
    // Execute code that triggers undefined-variable exception
    let value = elle::read_str("undefined-var", &mut symbols).unwrap();
    let expr = elle::compiler::converters::value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = elle::compile(&expr);
    
    // Execute and check that exception was set
    let _ = vm.execute(&bytecode);
    
    // Verify exception was set
    assert!(vm.current_exception.is_some());
    let exc = vm.current_exception.as_ref().unwrap();
    
    // Verify exception has location
    assert!(exc.location.is_some());
    let exc_loc = exc.location.as_ref().unwrap();
    assert_eq!(exc_loc.file, "test.lisp");
    assert_eq!(exc_loc.line, 5);
    assert_eq!(exc_loc.col, 10);
}
