// Point tests for the new compilation pipeline
//
// These tests cover semantic categories that don't lend themselves to property testing.
// They verify specific behaviors of the new Syntax → HIR → LIR → Bytecode pipeline.

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// 1. Shebang Handling
// ============================================================================
// The new pipeline's read_syntax / read_syntax_all handles shebangs at the
// reader level (see src/reader/mod.rs lines 44-50, 77-83).

#[test]
fn test_shebang_with_env_elle() {
    // Source starting with #!/usr/bin/env elle should evaluate correctly
    let result = eval_source("#!/usr/bin/env elle\n(+ 1 2)");
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_shebang_short_form() {
    // Source starting with #!elle should evaluate correctly
    let result = eval_source("#!elle\n42");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_no_shebang_works_normally() {
    // Source without shebang works normally
    let result = eval_source("(+ 10 20)");
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_shebang_with_complex_expression() {
    // Shebang followed by complex expression
    let result = eval_source("#!/usr/bin/env elle\n(let ((x 5)) (* x x))");
    assert_eq!(result.unwrap(), Value::int(25));
}

// ============================================================================
// 2. Macros
// ============================================================================
// The new pipeline uses Expander which supports defmacro (see src/syntax/expand.rs).
// However, macros defined in one form are not visible in subsequent forms when
// using eval because a fresh Expander is created for each compilation.
// The threading macros (-> and ->>) are built into the Expander.

#[test]
fn test_defmacro_my_when_true() {
    // Define a simple when macro and test with true condition
    let result = eval_source(
        "(begin
           (defmacro my-when (test body) `(if ,test ,body nil))
           (my-when true 42))",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_defmacro_my_when_false() {
    // Define a simple when macro and test with false condition
    let result = eval_source(
        "(begin
           (defmacro my-when (test body) `(if ,test ,body nil))
           (my-when false 42))",
    );
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_macro_predicate() {
    // Test macro? predicate after defining a macro
    // macro? is handled at expansion time - it checks if the symbol names a macro
    let result = eval_source(
        "(begin
           (defmacro my-when (test body) `(if ,test ,body nil))
           (macro? my-when))",
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_macro_predicate_non_macro() {
    // Test macro? predicate on a non-macro (built-in function)
    let result = eval_source("(macro? +)");
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_expand_macro() {
    // Test expand-macro returns the expanded form
    // expand-macro is handled at expansion time - it expands the quoted form
    // and returns the result as quoted data
    let result = eval_source(
        "(begin
           (defmacro my-when (test body) `(if ,test ,body nil))
           (expand-macro '(my-when true 42)))",
    );
    // Should return something like (if true 42 nil)
    assert!(result.is_ok());
    // Verify the expanded form is a list starting with 'if
    let expanded = result.unwrap();
    let items = expanded.list_to_vec().expect("should be a list");
    assert_eq!(items.len(), 4); // (if true 42 nil)
    assert!(items[0].is_symbol()); // 'if
}

// ============================================================================
// 3. Module-Qualified Names
// ============================================================================
// Module-qualified names: The lexer parses `module:name` as a single symbol,
// and the Expander resolves it to the flat primitive name at compile time.
// For example: string:upcase -> string-upcase, math:abs -> abs

#[test]
fn test_module_qualified_string_upcase() {
    // Test module-qualified syntax: string:upcase
    let result = eval_source("(string:upcase \"hello\")");
    assert_eq!(result.unwrap(), Value::string("HELLO"));
}

#[test]
fn test_module_qualified_math_abs() {
    // Test module-qualified syntax: math:abs
    let result = eval_source("(math:abs -5)");
    assert_eq!(result.unwrap(), Value::int(5));
}

// ============================================================================
// 4. Tables and Structs — Point Tests
// ============================================================================
// Note: The API uses polymorphic functions:
// - (get collection key [default]) - works on tables and structs
// - (put collection key value) - mutates tables, returns new struct
// - (keys collection), (values collection), (has-key? collection key)
// There are no table? or struct? predicates - use type-of instead.

#[test]
fn test_table_creation_empty() {
    // (table) creates empty table
    let result = eval_source("(table)").unwrap();
    assert!(result.is_table());
}

#[test]
fn test_table_put_and_get() {
    // (put table key value) then (get table key) returns value
    // Note: Tables use string keys, not keywords
    let result = eval_source(
        r#"(let ((t (table)))
           (put t "key" 42)
           (get t "key"))"#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_struct_creation_empty() {
    // (struct) creates empty struct
    let result = eval_source("(struct)").unwrap();
    assert!(result.is_struct());
}

#[test]
fn test_table_type_check() {
    // Verify table type using type_name() on Rust side
    let result = eval_source("(table)").unwrap();
    assert_eq!(result.type_name(), "table");
}

#[test]
fn test_struct_type_check() {
    // Verify struct type using type_name() on Rust side
    let result = eval_source("(struct)").unwrap();
    assert_eq!(result.type_name(), "struct");
}

#[test]
fn test_type_of_table() {
    // (type-of (table)) returns :table keyword
    // We verify by checking that (eq? (type-of (table)) :table) is true
    let result = eval_source("(eq? (type-of (table)) :table)");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_type_of_struct() {
    // (type-of (struct)) returns :struct keyword
    // We verify by checking that (eq? (type-of (struct)) :struct) is true
    let result = eval_source("(eq? (type-of (struct)) :struct)");
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_table_with_string_keys() {
    // Table with string key-value pairs
    let result = eval_source(
        r#"(let ((t (table "a" 1 "b" 2)))
           (+ (get t "a") (get t "b")))"#,
    );
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_struct_with_string_keys() {
    // Struct with string key-value pairs
    let result = eval_source(
        r#"(let ((s (struct "x" 10 "y" 20)))
           (+ (get s "x") (get s "y")))"#,
    );
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_table_has_key() {
    // Test has-key? on table
    let result = eval_source(
        r#"(let ((t (table "a" 1)))
           (has-key? t "a"))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_table_has_key_missing() {
    // Test has-key? on table for missing key
    let result = eval_source(
        r#"(let ((t (table "a" 1)))
           (has-key? t "b"))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(false));
}

// ============================================================================
// ============================================================================
// Additional Edge Cases
// ============================================================================

#[test]
fn test_table_mutation() {
    // Tables are mutable - put modifies in place
    let result = eval_source(
        r#"(let ((t (table)))
           (put t "a" 1)
           (put t "a" 2)
           (get t "a"))"#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_struct_immutability() {
    // Structs are immutable - put returns a new struct
    // We test that get works on initial values
    let result = eval_source(
        r#"(let ((s (struct "x" 42)))
           (get s "x"))"#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_nested_table_operations() {
    // Nested table operations
    let result = eval_source(
        r#"(let ((outer (table)))
           (put outer "inner" (table))
           (put (get outer "inner") "value" 42)
           (get (get outer "inner") "value"))"#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_defmacro_with_quasiquote() {
    // Macro using quasiquote for template
    let result = eval_source(
        "(begin
           (defmacro add-one (x) `(+ ,x 1))
           (add-one 41))",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_threading_macro_first() {
    // Thread-first macro (->) is built into the Expander
    let result = eval_source("(-> 5 (+ 3) (* 2))");
    // (-> 5 (+ 3) (* 2)) => (* (+ 5 3) 2) => (* 8 2) => 16
    assert_eq!(result.unwrap(), Value::int(16));
}

#[test]
fn test_threading_macro_last() {
    // Thread-last macro (->>) is built into the Expander
    let result = eval_source("(->> 5 (+ 3) (* 2))");
    // (->> 5 (+ 3) (* 2)) => (* 2 (+ 3 5)) => (* 2 8) => 16
    assert_eq!(result.unwrap(), Value::int(16));
}

#[test]
fn test_table_keys() {
    // Test keys function on table
    let result = eval_source(
        r#"(let ((t (table "a" 1 "b" 2)))
           (length (keys t)))"#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_table_values() {
    // Test values function on table
    let result = eval_source(
        r#"(let ((t (table "a" 1 "b" 2)))
           (length (values t)))"#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_table_del() {
    // Test del function on table (mutates in place)
    let result = eval_source(
        r#"(let ((t (table "a" 1 "b" 2)))
           (del t "a")
           (has-key? t "a"))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_struct_put_returns_new() {
    // Structs are immutable - put returns a new struct, original unchanged
    let result = eval_source(
        r#"(let ((s (struct "x" 1)))
           (let ((s2 (put s "x" 2)))
             (list (get s "x") (get s2 "x"))))"#,
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(1)); // Original unchanged
    assert_eq!(vec[1], Value::int(2)); // New struct has updated value
}

#[test]
fn test_get_with_default() {
    // Test get with default value for missing key
    let result = eval_source(
        r#"(let ((t (table)))
           (get t "missing" 42))"#,
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

// ============================================================================
// 7. Let Binding Semantics
// ============================================================================
// Standard Scheme `let` has parallel binding semantics: all init expressions
// are evaluated in the outer scope before any bindings are created.
// `let*` has sequential binding semantics: each binding can see previous ones.

#[test]
fn test_let_parallel_binding() {
    // Standard let: all init expressions evaluated in outer scope
    let result = eval_source("(let ((x 10) (y 20)) (+ x y))").unwrap();
    assert_eq!(result, Value::int(30));
}

#[test]
fn test_let_parallel_binding_shadowing() {
    // y should see the OUTER x (999), not the inner x (10)
    let result = eval_source("(begin (var x 999) (let ((x 10) (y x)) y))").unwrap();
    assert_eq!(result, Value::int(999));
}

#[test]
fn test_let_star_sequential_binding() {
    // let*: y should see the inner x (10)
    let result = eval_source("(begin (var x 999) (let* ((x 10) (y x)) y))").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_let_body_sees_bindings() {
    // Body should see the let bindings
    let result = eval_source("(let ((x 42)) x)").unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_let_nested_shadowing() {
    // Inner let shadows outer let
    let result = eval_source("(let ((x 1)) (let ((x 2)) x))").unwrap();
    assert_eq!(result, Value::int(2));
}
