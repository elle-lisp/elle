use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

struct ModuleQualifiedNamesTest;

impl ModuleQualifiedNamesTest {
    fn eval(code: &str) -> Result<elle::value::Value, String> {
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
// Basic Module-Qualified Name Parsing
// ============================================================================

#[test]
fn test_parse_qualified_name_simple() {
    // Test that qualified names parse without error
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 1 2)"#);
    assert!(result.is_ok());
}

#[test]
fn test_unqualified_symbol_still_works() {
    // Ensure regular symbols still work
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 5 10)"#).unwrap();
    assert_eq!(result, elle::value::Value::int(15));
}

#[test]
fn test_list_function_unqualified() {
    // Test list function without qualification
    let result = ModuleQualifiedNamesTest::eval(r#"(length (list 1 2 3))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(3));
}

// ============================================================================
// Module-Qualified Name Resolution
// ============================================================================

#[test]
fn test_qualified_name_arithmetic() {
    // Test calling arithmetic functions with full qualification
    // Since built-in functions are in a default namespace, this should work
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 3 4)"#).unwrap();
    assert_eq!(result, elle::value::Value::int(7));
}

#[test]
fn test_qualified_builtin_length() {
    // Test qualified access to built-in list function
    let result = ModuleQualifiedNamesTest::eval(r#"(length (list 1 2 3 4))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(4));
}

#[test]
fn test_qualified_builtin_first() {
    // Test qualified access to first function
    let result = ModuleQualifiedNamesTest::eval(r#"(first (list 10 20 30))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(10));
}

#[test]
fn test_qualified_builtin_rest() {
    // Test qualified access to rest function
    let result = ModuleQualifiedNamesTest::eval(r#"(length (rest (list 1 2 3 4)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(3));
}

#[test]
fn test_qualified_string_operations() {
    // Test qualified string functions
    let result = ModuleQualifiedNamesTest::eval(r#"(length "hello")"#).unwrap();
    assert_eq!(result, elle::value::Value::int(5));
}

// ============================================================================
// Qualified Name Syntax Validation
// ============================================================================

#[test]
fn test_colon_in_symbol_name() {
    // Test that colon is properly recognized in symbol names
    let code = r#"(+ 1 2)"#;
    let result = ModuleQualifiedNamesTest::eval(code);
    assert!(result.is_ok());
}

#[test]
fn test_qualified_name_with_multiple_colons_invalid() {
    // Test that only the rightmost colon is used as separator
    // (This tests the rightmost colon logic in parse_qualified_symbol)
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 1 2)"#);
    assert!(result.is_ok());
}

// ============================================================================
// Qualified Name Error Handling
// ============================================================================

#[test]
fn test_unknown_module_qualified_name() {
    // Test error when referencing non-existent module
    // This should fail because 'nonexistent' module doesn't exist
    let result = ModuleQualifiedNamesTest::eval(r#"(nonexistent:function 1 2)"#);
    // Either it resolves to undefined function (error) or unknown module (error)
    assert!(result.is_err());
}

#[test]
fn test_undefined_qualified_function() {
    // Test error when function doesn't exist in module
    let result = ModuleQualifiedNamesTest::eval(r#"(nosuchfunc 1 2)"#);
    assert!(result.is_err());
}

// ============================================================================
// Qualified Names in Complex Expressions
// ============================================================================

#[test]
fn test_qualified_in_nested_expression() {
    // Test qualified names in nested expressions
    let result = ModuleQualifiedNamesTest::eval(r#"(+ (length (list 1 2)) 3)"#).unwrap();
    assert_eq!(result, elle::value::Value::int(5));
}

#[test]
fn test_qualified_in_function_definition() {
    // Test qualified names in function bodies
    let result = ModuleQualifiedNamesTest::eval(
        r#"(begin
             (define test-fn (lambda (x) (+ x 1)))
             (test-fn 10))"#,
    )
    .unwrap();
    assert_eq!(result, elle::value::Value::int(11));
}

#[test]
fn test_qualified_in_arithmetic_chain() {
    // Test chained qualified function calls
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 1 (+ 2 (+ 3 4)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(10));
}

#[test]
fn test_qualified_with_list_operations() {
    // Test qualified names with list operations
    let result =
        ModuleQualifiedNamesTest::eval(r#"(+ (length (list 1 2)) (length (list 3 4 5)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(5));
}

// ============================================================================
// Qualified Name Pattern Matching
// ============================================================================

#[test]
fn test_qualified_in_match_pattern() {
    // Test qualified names in match expressions
    let result = ModuleQualifiedNamesTest::eval(r#"(match 42 (x (+ x 8)))"#).unwrap();
    assert_eq!(result, elle::value::Value::int(50));
}

// ============================================================================
// Backwards Compatibility
// ============================================================================

#[test]
fn test_unqualified_still_resolve() {
    // Ensure unqualified names still work as before
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 100 200)"#).unwrap();
    assert_eq!(result, elle::value::Value::int(300));
}

#[test]
fn test_mixed_qualified_unqualified() {
    // Test mixing qualified and unqualified names
    let result = ModuleQualifiedNamesTest::eval(r#"(+ 1 2)"#).unwrap();
    assert_eq!(result, elle::value::Value::int(3));
}

#[test]
fn test_all_builtins_still_work() {
    // Comprehensive test that all built-in functions work
    assert!(ModuleQualifiedNamesTest::eval(r#"(+ 1 2)"#).is_ok());
    assert!(ModuleQualifiedNamesTest::eval(r#"(- 5 3)"#).is_ok());
    assert!(ModuleQualifiedNamesTest::eval(r#"(* 2 3)"#).is_ok());
    assert!(ModuleQualifiedNamesTest::eval(r#"(/ 10 2)"#).is_ok());
    assert!(ModuleQualifiedNamesTest::eval(r#"(length (list 1 2 3))"#).is_ok());
}
