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

// ============================================================================
// 8. Polymorphic `get` - Unit A Tests
// ============================================================================
// Extend `get` to work on tuples, arrays, strings, structs, and tables

// Tuple (immutable indexed collection)
#[test]
fn test_get_tuple_by_index() {
    // (get [1 2 3] 0) → 1
    let result = eval_source("(get [1 2 3] 0)").unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn test_get_tuple_by_index_middle() {
    // (get [1 2 3] 1) → 2
    let result = eval_source("(get [1 2 3] 1)").unwrap();
    assert_eq!(result, Value::int(2));
}

#[test]
fn test_get_tuple_by_index_last() {
    // (get [1 2 3] 2) → 3
    let result = eval_source("(get [1 2 3] 2)").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn test_get_tuple_out_of_bounds_returns_default() {
    // (get [1 2 3] 10) → nil (default)
    let result = eval_source("(get [1 2 3] 10)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_tuple_out_of_bounds_with_default() {
    // (get [1 2 3] 10 :missing) → :missing
    let result = eval_source("(get [1 2 3] 10 :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_tuple_negative_index_returns_default() {
    // (get [1 2 3] -1) → nil (negative indices not supported)
    let result = eval_source("(get [1 2 3] -1)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_tuple_negative_index_with_default() {
    // (get [1 2 3] -1 :default) → :default
    let result = eval_source("(get [1 2 3] -1 :default)").unwrap();
    assert_eq!(result, Value::keyword("default"));
}

#[test]
fn test_get_empty_tuple_returns_default() {
    // (get [] 0) → nil
    let result = eval_source("(get [] 0)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_tuple_non_integer_index_error() {
    // (get [1 2 3] :key) → error
    let result = eval_source("(get [1 2 3] :key)");
    assert!(result.is_err());
}

// Array (mutable indexed collection)
#[test]
fn test_get_array_by_index() {
    // (get @[1 2 3] 0) → 1
    let result = eval_source("(get @[1 2 3] 0)").unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn test_get_array_by_index_middle() {
    // (get @[1 2 3] 1) → 2
    let result = eval_source("(get @[1 2 3] 1)").unwrap();
    assert_eq!(result, Value::int(2));
}

#[test]
fn test_get_array_by_index_last() {
    // (get @[1 2 3] 2) → 3
    let result = eval_source("(get @[1 2 3] 2)").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn test_get_array_out_of_bounds_returns_default() {
    // (get @[1 2 3] 10) → nil
    let result = eval_source("(get @[1 2 3] 10)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_array_out_of_bounds_with_default() {
    // (get @[1 2 3] 10 :missing) → :missing
    let result = eval_source("(get @[1 2 3] 10 :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_array_negative_index_returns_default() {
    // (get @[1 2 3] -1) → nil
    let result = eval_source("(get @[1 2 3] -1)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_empty_array_returns_default() {
    // (get @[] 0) → nil
    let result = eval_source("(get @[] 0)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_array_non_integer_index_error() {
    // (get @[1 2 3] :key) → error
    let result = eval_source("(get @[1 2 3] :key)");
    assert!(result.is_err());
}

// String (immutable character sequence)
#[test]
fn test_get_string_by_char_index() {
    // (get "hello" 0) → "h"
    let result = eval_source("(get \"hello\" 0)").unwrap();
    assert_eq!(result, Value::string("h"));
}

#[test]
fn test_get_string_by_char_index_middle() {
    // (get "hello" 1) → "e"
    let result = eval_source("(get \"hello\" 1)").unwrap();
    assert_eq!(result, Value::string("e"));
}

#[test]
fn test_get_string_by_char_index_last() {
    // (get "hello" 4) → "o"
    let result = eval_source("(get \"hello\" 4)").unwrap();
    assert_eq!(result, Value::string("o"));
}

#[test]
fn test_get_string_out_of_bounds_returns_default() {
    // (get "hello" 10) → nil
    let result = eval_source("(get \"hello\" 10)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_string_out_of_bounds_with_default() {
    // (get "hello" 10 :missing) → :missing
    let result = eval_source("(get \"hello\" 10 :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_string_negative_index_returns_default() {
    // (get "hello" -1) → nil
    let result = eval_source("(get \"hello\" -1)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_empty_string_returns_default() {
    // (get "" 0) → nil
    let result = eval_source("(get \"\" 0)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_string_non_integer_index_error() {
    // (get "hello" :key) → error
    let result = eval_source("(get \"hello\" :key)");
    assert!(result.is_err());
}

#[test]
fn test_get_string_unicode_char() {
    // (get "café" 3) → "é" (UTF-8 character)
    let result = eval_source("(get \"café\" 3)").unwrap();
    assert_eq!(result, Value::string("é"));
}

// Struct (immutable keyed collection)
#[test]
fn test_get_struct_by_keyword() {
    // (get {:a 1} :a) → 1
    let result = eval_source("(get {:a 1} :a)").unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn test_get_struct_by_string_key() {
    // (get {"x" 10} "x") → 10
    let result = eval_source("(get {\"x\" 10} \"x\")").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_get_struct_missing_key_returns_default() {
    // (get {:a 1} :b) → nil
    let result = eval_source("(get {:a 1} :b)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_struct_missing_key_with_default() {
    // (get {:a 1} :b :missing) → :missing
    let result = eval_source("(get {:a 1} :b :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_empty_struct_returns_default() {
    // (get {} :a) → nil
    let result = eval_source("(get {} :a)").unwrap();
    assert_eq!(result, Value::NIL);
}

// Table (mutable keyed collection)
#[test]
fn test_get_table_by_keyword() {
    // (get @{:a 1} :a) → 1
    let result = eval_source("(get @{:a 1} :a)").unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn test_get_table_by_string_key() {
    // (get @{"x" 10} "x") → 10
    let result = eval_source("(get @{\"x\" 10} \"x\")").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_get_table_missing_key_returns_default() {
    // (get @{:a 1} :b) → nil
    let result = eval_source("(get @{:a 1} :b)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_table_missing_key_with_default() {
    // (get @{:a 1} :b :missing) → :missing
    let result = eval_source("(get @{:a 1} :b :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_empty_table_returns_default() {
    // (get @{} :a) → nil
    let result = eval_source("(get @{} :a)").unwrap();
    assert_eq!(result, Value::NIL);
}

// Error cases
#[test]
fn test_get_wrong_arity_no_args() {
    // (get) → error
    let result = eval_source("(get)");
    assert!(result.is_err());
}

#[test]
fn test_get_wrong_arity_one_arg() {
    // (get [1 2 3]) → error
    let result = eval_source("(get [1 2 3])");
    assert!(result.is_err());
}

#[test]
fn test_get_wrong_arity_too_many_args() {
    // (get [1 2 3] 0 :default :extra) → error
    let result = eval_source("(get [1 2 3] 0 :default :extra)");
    assert!(result.is_err());
}

#[test]
fn test_get_unsupported_type() {
    // (get 42 0) → error (integers are not collections)
    let result = eval_source("(get 42 0)");
    assert!(result.is_err());
}

#[test]
fn test_get_nil_type() {
    // (get nil 0) → error
    let result = eval_source("(get nil 0)");
    assert!(result.is_err());
}

// ============================================================================
// Tuple (immutable indexed collection) - returns new tuple
// ============================================================================

#[test]
fn test_put_tuple_by_index() {
    // (put [1 2 3] 0 99) → [99 2 3]
    let result = eval_source("(put [1 2 3] 0 99)").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(99));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_tuple_by_index_middle() {
    // (put [1 2 3] 1 99) → [1 99 3]
    let result = eval_source("(put [1 2 3] 1 99)").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(99));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_tuple_by_index_last() {
    // (put [1 2 3] 2 99) → [1 2 99]
    let result = eval_source("(put [1 2 3] 2 99)").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(99));
}

#[test]
fn test_put_tuple_out_of_bounds_errors() {
    // (put [1 2 3] 10 99) → error (out of bounds)
    let result = eval_source("(put [1 2 3] 10 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_tuple_negative_index_errors() {
    // (put [1 2 3] -1 99) → error (negative index)
    let result = eval_source("(put [1 2 3] -1 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_tuple_immutable_original_unchanged() {
    // Original tuple should be unchanged
    let result = eval_source(
        r#"(let ((t [1 2 3]))
             (let ((t2 (put t 0 99)))
               (list t t2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_tuple().unwrap();
    let modified = list[1].as_tuple().unwrap();
    assert_eq!(orig[0], Value::int(1)); // Original unchanged
    assert_eq!(modified[0], Value::int(99)); // New tuple modified
}

#[test]
fn test_put_tuple_non_integer_index_error() {
    // (put [1 2 3] :key 99) → error
    let result = eval_source("(put [1 2 3] :key 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_empty_tuple_errors() {
    // (put [] 0 99) → error (out of bounds on empty tuple)
    let result = eval_source("(put [] 0 99)");
    assert!(result.is_err());
}

// ============================================================================
// Array (mutable indexed collection) - mutates in place, returns array
// ============================================================================

#[test]
fn test_put_array_by_index() {
    // (put @[1 2 3] 0 99) → @[99 2 3] (mutates in place)
    let result = eval_source("(put @[1 2 3] 0 99)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(99));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_array_by_index_middle() {
    // (put @[1 2 3] 1 99) → @[1 99 3]
    let result = eval_source("(put @[1 2 3] 1 99)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(99));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_array_by_index_last() {
    // (put @[1 2 3] 2 99) → @[1 2 99]
    let result = eval_source("(put @[1 2 3] 2 99)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(99));
}

#[test]
fn test_put_array_out_of_bounds_errors() {
    // (put @[1 2 3] 10 99) → error (out of bounds)
    let result = eval_source("(put @[1 2 3] 10 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_array_negative_index_errors() {
    // (put @[1 2 3] -1 99) → error (negative index)
    let result = eval_source("(put @[1 2 3] -1 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_array_mutable_same_reference() {
    // put returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 2 3]))
             (let ((a2 (put a 0 99)))
               (eq? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_put_array_non_integer_index_error() {
    // (put @[1 2 3] :key 99) → error
    let result = eval_source("(put @[1 2 3] :key 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_empty_array_errors() {
    // (put @[] 0 99) → error (out of bounds on empty array)
    let result = eval_source("(put @[] 0 99)");
    assert!(result.is_err());
}

// ============================================================================
// String (immutable character sequence) - returns new string
// ============================================================================

#[test]
fn test_put_string_by_char_index() {
    // (put "hello" 0 "a") → "aello"
    let result = eval_source("(put \"hello\" 0 \"a\")").unwrap();
    assert_eq!(result, Value::string("aello"));
}

#[test]
fn test_put_string_by_char_index_middle() {
    // (put "hello" 1 "a") → "hallo"
    let result = eval_source("(put \"hello\" 1 \"a\")").unwrap();
    assert_eq!(result, Value::string("hallo"));
}

#[test]
fn test_put_string_by_char_index_last() {
    // (put "hello" 4 "a") → "hella"
    let result = eval_source("(put \"hello\" 4 \"a\")").unwrap();
    assert_eq!(result, Value::string("hella"));
}

#[test]
fn test_put_string_out_of_bounds_errors() {
    // (put "hello" 10 "a") → error (out of bounds)
    let result = eval_source("(put \"hello\" 10 \"a\")");
    assert!(result.is_err());
}

#[test]
fn test_put_string_negative_index_errors() {
    // (put "hello" -1 "a") → error (negative index)
    let result = eval_source("(put \"hello\" -1 \"a\")");
    assert!(result.is_err());
}

#[test]
fn test_put_string_immutable_original_unchanged() {
    // Original string should be unchanged
    let result = eval_source(
        r#"(let ((s "hello"))
             (let ((s2 (put s 0 "a")))
               (list s s2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::string("hello")); // Original unchanged
    assert_eq!(list[1], Value::string("aello")); // New string modified
}

#[test]
fn test_put_string_non_integer_index_error() {
    // (put "hello" :key "a") → error
    let result = eval_source("(put \"hello\" :key \"a\")");
    assert!(result.is_err());
}

#[test]
fn test_put_empty_string_errors() {
    // (put "" 0 "a") → error (out of bounds on empty string)
    let result = eval_source("(put \"\" 0 \"a\")");
    assert!(result.is_err());
}

#[test]
fn test_put_string_unicode_char() {
    // (put "café" 3 "x") → "cafx" (UTF-8 character replacement, é is at index 3)
    let result = eval_source("(put \"café\" 3 \"x\")").unwrap();
    assert_eq!(result, Value::string("cafx"));
}

// ============================================================================
// Struct (immutable keyed collection) - returns new struct
// ============================================================================

#[test]
fn test_put_struct_by_keyword() {
    // (put {:a 1} :a 99) → {:a 99}
    let result = eval_source("(put {:a 1} :a 99)").unwrap();
    assert!(result.is_struct());
    let val = eval_source("(get (put {:a 1} :a 99) :a)").unwrap();
    assert_eq!(val, Value::int(99));
}

#[test]
fn test_put_struct_new_key() {
    // (put {:a 1} :b 2) → {:a 1 :b 2}
    let result = eval_source("(put {:a 1} :b 2)").unwrap();
    assert!(result.is_struct());
    let a_val = eval_source("(get (put {:a 1} :b 2) :a)").unwrap();
    let b_val = eval_source("(get (put {:a 1} :b 2) :b)").unwrap();
    assert_eq!(a_val, Value::int(1));
    assert_eq!(b_val, Value::int(2));
}

#[test]
fn test_put_struct_immutable_original_unchanged() {
    // Original struct should be unchanged
    let result = eval_source(
        r#"(let ((s {:a 1}))
             (let ((s2 (put s :a 99)))
               (list (get s :a) (get s2 :a))))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::int(1)); // Original unchanged
    assert_eq!(list[1], Value::int(99)); // New struct modified
}

#[test]
fn test_put_empty_struct() {
    // (put {} :a 1) → {:a 1}
    let _result = eval_source("(put {} :a 1)").unwrap();
    let val = eval_source("(get (put {} :a 1) :a)").unwrap();
    assert_eq!(val, Value::int(1));
}

// ============================================================================
// Table (mutable keyed collection) - mutates in place, returns table
// ============================================================================

#[test]
fn test_put_table_by_keyword() {
    // (put @{:a 1} :a 99) → @{:a 99} (mutates in place)
    let result = eval_source("(put @{:a 1} :a 99)").unwrap();
    assert!(result.is_table());
    let val = eval_source("(get (put @{:a 1} :a 99) :a)").unwrap();
    assert_eq!(val, Value::int(99));
}

#[test]
fn test_put_table_new_key() {
    // (put @{:a 1} :b 2) → @{:a 1 :b 2}
    let result = eval_source("(put @{:a 1} :b 2)").unwrap();
    assert!(result.is_table());
    let a_val = eval_source("(get (put @{:a 1} :b 2) :a)").unwrap();
    let b_val = eval_source("(get (put @{:a 1} :b 2) :b)").unwrap();
    assert_eq!(a_val, Value::int(1));
    assert_eq!(b_val, Value::int(2));
}

#[test]
fn test_put_table_mutable_same_reference() {
    // put returns the same table (mutated in place)
    let result = eval_source(
        r#"(let ((t @{:a 1}))
             (let ((t2 (put t :a 99)))
               (eq? t t2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_put_empty_table() {
    // (put @{} :a 1) → @{:a 1}
    let _result = eval_source("(put @{} :a 1)").unwrap();
    let val = eval_source("(get (put @{} :a 1) :a)").unwrap();
    assert_eq!(val, Value::int(1));
}

// ============================================================================
// Error Cases
// ============================================================================

#[test]
fn test_put_wrong_arity_no_args() {
    // (put) → error
    let result = eval_source("(put)");
    assert!(result.is_err());
}

#[test]
fn test_put_wrong_arity_one_arg() {
    // (put [1 2 3]) → error
    let result = eval_source("(put [1 2 3])");
    assert!(result.is_err());
}

#[test]
fn test_put_wrong_arity_two_args() {
    // (put [1 2 3] 0) → error
    let result = eval_source("(put [1 2 3] 0)");
    assert!(result.is_err());
}

#[test]
fn test_put_wrong_arity_too_many_args() {
    // (put [1 2 3] 0 99 :extra) → error
    let result = eval_source("(put [1 2 3] 0 99 :extra)");
    assert!(result.is_err());
}

#[test]
fn test_put_unsupported_type() {
    // (put 42 0 99) → error (integers are not collections)
    let result = eval_source("(put 42 0 99)");
    assert!(result.is_err());
}

#[test]
fn test_put_nil_type() {
    // (put nil 0 99) → error
    let result = eval_source("(put nil 0 99)");
    assert!(result.is_err());
}

// ============================================================================
// rebox - replace box-set!
// ============================================================================

#[test]
fn test_rebox_returns_value() {
    // (rebox (box 1) 42) → 42 (returns the value, not the cell)
    let result = eval_source("(rebox (box 1) 42)").unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_rebox_updates_cell() {
    // (var b (box 1)) (rebox b 2) (unbox b) → 2
    let result = eval_source(
        r#"(var b (box 1))
           (rebox b 2)
           (unbox b)"#,
    );
    assert_eq!(result.unwrap(), Value::int(2));
}

#[test]
fn test_rebox_with_different_types() {
    // (var b (box 1)) (rebox b "hello") (unbox b) → "hello"
    let result = eval_source(
        r#"(var b (box 1))
           (rebox b "hello")
           (unbox b)"#,
    );
    assert_eq!(result.unwrap(), Value::string("hello"));
}

#[test]
fn test_rebox_with_nil() {
    // (var b (box 1)) (rebox b nil) (unbox b) → nil
    let result = eval_source(
        r#"(var b (box 1))
           (rebox b nil)
           (unbox b)"#,
    );
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_rebox_wrong_arity_no_args() {
    // (rebox) → error
    let result = eval_source("(rebox)");
    assert!(result.is_err());
}

#[test]
fn test_rebox_wrong_arity_one_arg() {
    // (rebox (box 1)) → error
    let result = eval_source("(rebox (box 1))");
    assert!(result.is_err());
}

#[test]
fn test_rebox_wrong_arity_too_many_args() {
    // (rebox (box 1) 2 3) → error
    let result = eval_source("(rebox (box 1) 2 3)");
    assert!(result.is_err());
}

#[test]
fn test_rebox_non_cell_error() {
    // (rebox 42 99) → error (not a cell)
    let result = eval_source("(rebox 42 99)");
    assert!(result.is_err());
}

// ============================================================================
// push - add element to end of array
// ============================================================================

#[test]
fn test_push_single_element() {
    // (push @[1 2] 3) → @[1 2 3]
    let result = eval_source("(push @[1 2] 3)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_push_returns_same_array() {
    // push returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 2]))
             (let ((a2 (push a 3)))
               (eq? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_push_empty_array() {
    // (push @[] 1) → @[1]
    let result = eval_source("(push @[] 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_push_multiple_times() {
    // (var a @[]) (push a 1) (push a 2) (push a 3) a → @[1 2 3]
    let result = eval_source(
        r#"(var a @[])
           (push a 1)
           (push a 2)
           (push a 3)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_push_wrong_arity_no_args() {
    // (push) → error
    let result = eval_source("(push)");
    assert!(result.is_err());
}

#[test]
fn test_push_wrong_arity_one_arg() {
    // (push @[1 2]) → error
    let result = eval_source("(push @[1 2])");
    assert!(result.is_err());
}

#[test]
fn test_push_wrong_arity_too_many_args() {
    // (push @[1 2] 3 4) → error
    let result = eval_source("(push @[1 2] 3 4)");
    assert!(result.is_err());
}

#[test]
fn test_push_non_array_error() {
    // (push [1 2] 3) → error (tuple, not array)
    let result = eval_source("(push [1 2] 3)");
    assert!(result.is_err());
}

// ============================================================================
// pop - remove and return last element
// ============================================================================

#[test]
fn test_pop_single_element() {
    // (pop @[1 2 3]) → 3
    let result = eval_source("(pop @[1 2 3])").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn test_pop_mutates_array() {
    // (var a @[1 2 3]) (pop a) a → @[1 2]
    let result = eval_source(
        r#"(var a @[1 2 3])
           (pop a)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_pop_empty_array_errors() {
    // (pop @[]) → error
    let result = eval_source("(pop @[])");
    assert!(result.is_err());
}

#[test]
fn test_pop_single_element_array() {
    // (var a @[42]) (pop a) a → @[]
    let result = eval_source(
        r#"(var a @[42])
           (pop a)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_pop_wrong_arity_no_args() {
    // (pop) → error
    let result = eval_source("(pop)");
    assert!(result.is_err());
}

#[test]
fn test_pop_wrong_arity_too_many_args() {
    // (pop @[1 2] 3) → error
    let result = eval_source("(pop @[1 2] 3)");
    assert!(result.is_err());
}

#[test]
fn test_pop_non_array_error() {
    // (pop [1 2 3]) → error (tuple, not array)
    let result = eval_source("(pop [1 2 3])");
    assert!(result.is_err());
}

// ============================================================================
// popn - remove and return last n elements as new array
// ============================================================================

#[test]
fn test_popn_two_elements() {
    // (popn @[1 2 3 4] 2) → @[3 4]
    let result = eval_source("(popn @[1 2 3 4] 2)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(3));
    assert_eq!(vec[1], Value::int(4));
}

#[test]
fn test_popn_mutates_original() {
    // (var a @[1 2 3 4]) (popn a 2) a → @[1 2]
    let result = eval_source(
        r#"(var a @[1 2 3 4])
           (popn a 2)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_popn_all_elements() {
    // (var a @[1 2 3]) (popn a 3) a → @[]
    let result = eval_source(
        r#"(var a @[1 2 3])
           (popn a 3)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_more_than_available() {
    // (var a @[1 2]) (popn a 5) a → @[] (removes all)
    let result = eval_source(
        r#"(var a @[1 2])
           (popn a 5)
           a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_zero_elements() {
    // (popn @[1 2 3] 0) → @[]
    let result = eval_source("(popn @[1 2 3] 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_empty_array() {
    // (popn @[] 2) → @[]
    let result = eval_source("(popn @[] 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_wrong_arity_no_args() {
    // (popn) → error
    let result = eval_source("(popn)");
    assert!(result.is_err());
}

#[test]
fn test_popn_wrong_arity_one_arg() {
    // (popn @[1 2 3]) → error
    let result = eval_source("(popn @[1 2 3])");
    assert!(result.is_err());
}

#[test]
fn test_popn_wrong_arity_too_many_args() {
    // (popn @[1 2 3] 2 3) → error
    let result = eval_source("(popn @[1 2 3] 2 3)");
    assert!(result.is_err());
}

#[test]
fn test_popn_non_integer_count_error() {
    // (popn @[1 2 3] :key) → error
    let result = eval_source("(popn @[1 2 3] :key)");
    assert!(result.is_err());
}

#[test]
fn test_popn_non_array_error() {
    // (popn [1 2 3] 2) → error (tuple, not array)
    let result = eval_source("(popn [1 2 3] 2)");
    assert!(result.is_err());
}

// ============================================================================
// insert - insert element at index
// ============================================================================

#[test]
fn test_insert_at_beginning() {
    // (insert @[2 3] 0 1) → @[1 2 3]
    let result = eval_source("(insert @[2 3] 0 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_at_middle() {
    // (insert @[1 3] 1 2) → @[1 2 3]
    let result = eval_source("(insert @[1 3] 1 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_at_end() {
    // (insert @[1 2] 2 3) → @[1 2 3]
    let result = eval_source("(insert @[1 2] 2 3)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_returns_same_array() {
    // insert returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 3]))
             (let ((a2 (insert a 1 2)))
               (eq? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_insert_empty_array() {
    // (insert @[] 0 1) → @[1]
    let result = eval_source("(insert @[] 0 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_insert_out_of_bounds_appends() {
    // (insert @[1 2] 10 3) → @[1 2 3] (out of bounds, appends)
    let result = eval_source("(insert @[1 2] 10 3)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_wrong_arity_no_args() {
    // (insert) → error
    let result = eval_source("(insert)");
    assert!(result.is_err());
}

#[test]
fn test_insert_wrong_arity_one_arg() {
    // (insert @[1 2]) → error
    let result = eval_source("(insert @[1 2])");
    assert!(result.is_err());
}

#[test]
fn test_insert_wrong_arity_two_args() {
    // (insert @[1 2] 0) → error
    let result = eval_source("(insert @[1 2] 0)");
    assert!(result.is_err());
}

#[test]
fn test_insert_wrong_arity_too_many_args() {
    // (insert @[1 2] 0 3 4) → error
    let result = eval_source("(insert @[1 2] 0 3 4)");
    assert!(result.is_err());
}

#[test]
fn test_insert_non_integer_index_error() {
    // (insert @[1 2] :key 3) → error
    let result = eval_source("(insert @[1 2] :key 3)");
    assert!(result.is_err());
}

#[test]
fn test_insert_non_array_error() {
    // (insert [1 2] 0 3) → error (tuple, not array)
    let result = eval_source("(insert [1 2] 0 3)");
    assert!(result.is_err());
}

// ============================================================================
// remove - remove element at index
// ============================================================================

#[test]
fn test_remove_at_beginning() {
    // (remove @[1 2 3] 0) → @[2 3]
    let result = eval_source("(remove @[1 2 3] 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(2));
    assert_eq!(vec[1], Value::int(3));
}

#[test]
fn test_remove_at_middle() {
    // (remove @[1 2 3] 1) → @[1 3]
    let result = eval_source("(remove @[1 2 3] 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(3));
}

#[test]
fn test_remove_at_end() {
    // (remove @[1 2 3] 2) → @[1 2]
    let result = eval_source("(remove @[1 2 3] 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_remove_with_count() {
    // (remove @[1 2 3 4] 1 2) → @[1 4] (remove 2 elements starting at index 1)
    let result = eval_source("(remove @[1 2 3 4] 1 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(4));
}

#[test]
fn test_remove_returns_same_array() {
    // remove returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 2 3]))
             (let ((a2 (remove a 1)))
               (eq? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_remove_out_of_bounds_no_change() {
    // (remove @[1 2 3] 10) → @[1 2 3] (out of bounds, no change)
    let result = eval_source("(remove @[1 2 3] 10)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_remove_count_exceeds_available() {
    // (remove @[1 2 3] 1 10) → @[1] (remove all from index 1 onward)
    let result = eval_source("(remove @[1 2 3] 1 10)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_remove_zero_count() {
    // (remove @[1 2 3] 1 0) → @[1 2 3] (remove 0 elements, no change)
    let result = eval_source("(remove @[1 2 3] 1 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_remove_wrong_arity_no_args() {
    // (remove) → error
    let result = eval_source("(remove)");
    assert!(result.is_err());
}

#[test]
fn test_remove_wrong_arity_one_arg() {
    // (remove @[1 2 3]) → error
    let result = eval_source("(remove @[1 2 3])");
    assert!(result.is_err());
}

#[test]
fn test_remove_wrong_arity_too_many_args() {
    // (remove @[1 2 3] 0 1 2) → error
    let result = eval_source("(remove @[1 2 3] 0 1 2)");
    assert!(result.is_err());
}

#[test]
fn test_remove_non_integer_index_error() {
    // (remove @[1 2 3] :key) → error
    let result = eval_source("(remove @[1 2 3] :key)");
    assert!(result.is_err());
}

#[test]
fn test_remove_non_integer_count_error() {
    // (remove @[1 2 3] 0 :key) → error
    let result = eval_source("(remove @[1 2 3] 0 :key)");
    assert!(result.is_err());
}

#[test]
fn test_remove_non_array_error() {
    // (remove [1 2 3] 0) → error (tuple, not array)
    let result = eval_source("(remove [1 2 3] 0)");
    assert!(result.is_err());
}

// ============================================================================
// append - polymorphic, mutates mutable types, returns new for immutable
// ============================================================================

#[test]
fn test_append_arrays_mutates() {
    // (append @[1 2] @[3 4]) → same array, now @[1 2 3 4]
    let result = eval_source("(append @[1 2] @[3 4])").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_arrays_returns_same_reference() {
    // append returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 2]))
             (let ((a2 (append a @[3 4])))
               (eq? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_append_tuples_returns_new() {
    // (append [1 2] [3 4]) → new tuple [1 2 3 4]
    let result = eval_source("(append [1 2] [3 4])").unwrap();
    assert!(result.is_tuple());
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_tuples_original_unchanged() {
    // Original tuple should be unchanged
    let result = eval_source(
        r#"(let ((t [1 2]))
             (let ((t2 (append t [3 4])))
               (list t t2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_tuple().unwrap();
    let appended = list[1].as_tuple().unwrap();
    assert_eq!(orig.len(), 2); // Original unchanged
    assert_eq!(appended.len(), 4); // New tuple has both
}

#[test]
fn test_append_strings() {
    // (append "hello" " world") → "hello world"
    let result = eval_source("(append \"hello\" \" world\")").unwrap();
    assert_eq!(result, Value::string("hello world"));
}

#[test]
fn test_append_strings_returns_new() {
    // String append returns new string (immutable)
    let result = eval_source(
        r#"(let ((s "hello"))
             (let ((s2 (append s " world")))
               (list s s2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::string("hello")); // Original unchanged
    assert_eq!(list[1], Value::string("hello world")); // New string
}

#[test]
fn test_append_empty_arrays() {
    // (append @[] @[1 2]) → @[1 2]
    let result = eval_source("(append @[] @[1 2])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_to_empty_array() {
    // (append @[1 2] @[]) → @[1 2]
    let result = eval_source("(append @[1 2] @[])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_empty_tuples() {
    // (append [] [1 2]) → [1 2]
    let result = eval_source("(append [] [1 2])").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_append_empty_strings() {
    // (append "" "hello") → "hello"
    let result = eval_source("(append \"\" \"hello\")").unwrap();
    assert_eq!(result, Value::string("hello"));
}

#[test]
fn test_append_to_empty_string() {
    // (append "hello" "") → "hello"
    let result = eval_source("(append \"hello\" \"\")").unwrap();
    assert_eq!(result, Value::string("hello"));
}

#[test]
fn test_append_wrong_arity_no_args() {
    // (append) → error
    let result = eval_source("(append)");
    assert!(result.is_err());
}

#[test]
fn test_append_wrong_arity_one_arg() {
    // (append @[1 2]) → error
    let result = eval_source("(append @[1 2])");
    assert!(result.is_err());
}

#[test]
fn test_append_wrong_arity_too_many_args() {
    // (append @[1 2] @[3 4] @[5 6]) → error
    let result = eval_source("(append @[1 2] @[3 4] @[5 6])");
    assert!(result.is_err());
}

#[test]
fn test_append_mismatched_types_error() {
    // (append @[1 2] [3 4]) → error (array and tuple)
    let result = eval_source("(append @[1 2] [3 4])");
    assert!(result.is_err());
}

#[test]
fn test_append_unsupported_type_error() {
    // (append 42 99) → error
    let result = eval_source("(append 42 99)");
    assert!(result.is_err());
}

// ============================================================================
// concat - always returns new value, never mutates
// ============================================================================

#[test]
fn test_concat_arrays_returns_new() {
    // (concat @[1 2] @[3 4]) → new array @[1 2 3 4]
    let result = eval_source("(concat @[1 2] @[3 4])").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_concat_arrays_original_unchanged() {
    // Original arrays should be unchanged
    let result = eval_source(
        r#"(let ((a @[1 2]))
             (let ((a2 (concat a @[3 4])))
               (list a a2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_array().unwrap().borrow();
    let concatenated = list[1].as_array().unwrap().borrow();
    assert_eq!(orig.len(), 2); // Original unchanged
    assert_eq!(concatenated.len(), 4); // New array has both
}

#[test]
fn test_concat_tuples_returns_new() {
    // (concat [1 2] [3 4]) → new tuple [1 2 3 4]
    let result = eval_source("(concat [1 2] [3 4])").unwrap();
    assert!(result.is_tuple());
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 4);
}

#[test]
fn test_concat_strings() {
    // (concat "hello" " world") → "hello world"
    let result = eval_source("(concat \"hello\" \" world\")").unwrap();
    assert_eq!(result, Value::string("hello world"));
}

#[test]
fn test_concat_empty_arrays() {
    // (concat @[] @[1 2]) → @[1 2]
    let result = eval_source("(concat @[] @[1 2])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_concat_to_empty_array() {
    // (concat @[1 2] @[]) → @[1 2]
    let result = eval_source("(concat @[1 2] @[])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_concat_empty_tuples() {
    // (concat [] [1 2]) → [1 2]
    let result = eval_source("(concat [] [1 2])").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_concat_empty_strings() {
    // (concat "" "hello") → "hello"
    let result = eval_source("(concat \"\" \"hello\")").unwrap();
    assert_eq!(result, Value::string("hello"));
}

#[test]
fn test_concat_wrong_arity_no_args() {
    // (concat) → error
    let result = eval_source("(concat)");
    assert!(result.is_err());
}

#[test]
fn test_concat_wrong_arity_one_arg() {
    // (concat @[1 2]) → error
    let result = eval_source("(concat @[1 2])");
    assert!(result.is_err());
}

#[test]
fn test_concat_wrong_arity_too_many_args() {
    // (concat @[1 2] @[3 4] @[5 6]) → error
    let result = eval_source("(concat @[1 2] @[3 4] @[5 6])");
    assert!(result.is_err());
}

#[test]
fn test_concat_mismatched_types_error() {
    // (concat @[1 2] [3 4]) → error (array and tuple)
    let result = eval_source("(concat @[1 2] [3 4])");
    assert!(result.is_err());
}

#[test]
fn test_concat_unsupported_type_error() {
    // (concat 42 99) → error
    let result = eval_source("(concat 42 99)");
    assert!(result.is_err());
}

// ============================================================================
// get on lists (cons-based)
// ============================================================================

#[test]
fn test_get_list_by_index() {
    // (get (list 10 20 30) 0) → 10
    let result = eval_source("(get (list 10 20 30) 0)").unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn test_get_list_by_index_middle() {
    // (get (list 10 20 30) 1) → 20
    let result = eval_source("(get (list 10 20 30) 1)").unwrap();
    assert_eq!(result, Value::int(20));
}

#[test]
fn test_get_list_by_index_last() {
    // (get (list 10 20 30) 2) → 30
    let result = eval_source("(get (list 10 20 30) 2)").unwrap();
    assert_eq!(result, Value::int(30));
}

#[test]
fn test_get_list_out_of_bounds_returns_default() {
    // (get (list 10 20 30) 10) → nil
    let result = eval_source("(get (list 10 20 30) 10)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_list_out_of_bounds_with_default() {
    // (get (list 10 20 30) 10 :missing) → :missing
    let result = eval_source("(get (list 10 20 30) 10 :missing)").unwrap();
    assert_eq!(result, Value::keyword("missing"));
}

#[test]
fn test_get_list_negative_index_returns_default() {
    // (get (list 10 20 30) -1) → nil
    let result = eval_source("(get (list 10 20 30) -1)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_empty_list_returns_default() {
    // (get (list) 0) → nil
    let result = eval_source("(get (list) 0)").unwrap();
    assert_eq!(result, Value::NIL);
}

#[test]
fn test_get_list_non_integer_index_error() {
    // (get (list 1 2 3) :key) → error
    let result = eval_source("(get (list 1 2 3) :key)");
    assert!(result.is_err());
}

// ============================================================================
// append on lists (cons-based)
// ============================================================================

#[test]
fn test_append_lists() {
    // (append (list 1 2) (list 3 4)) → (1 2 3 4)
    let result = eval_source("(append (list 1 2) (list 3 4))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_empty_list_to_list() {
    // (append (list) (list 1 2)) → (1 2)
    let result = eval_source("(append (list) (list 1 2))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_list_to_empty_list() {
    // (append (list 1 2) (list)) → (1 2)
    let result = eval_source("(append (list 1 2) (list))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_empty_lists() {
    // (append (list) (list)) → ()
    let result = eval_source("(append (list) (list))").unwrap();
    assert!(result.is_empty_list());
}

#[test]
fn test_append_lists_mismatched_type_error() {
    // (append (list 1 2) @[3 4]) → error
    let result = eval_source("(append (list 1 2) @[3 4])");
    assert!(result.is_err());
}
