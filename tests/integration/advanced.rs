// DEFENSE: Integration tests ensure the full pipeline works end-to-end
use crate::common::eval_source;
use elle::Value;

// Phase 5: Advanced Runtime Features - Integration Tests

#[test]
fn test_import_file_integration() {
    // Test that import-file is available and callable with a valid file
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());

    // Non-existent files should return an error
    assert!(eval_source("(import-file \"./lib/nonexistent.lisp\")").is_err());
    assert!(eval_source("(import-file \"/absolute/nonexistent.lisp\")").is_err());
}

#[test]
fn test_spawn_and_thread_id() {
    let result = eval_source("(current-thread-id)").unwrap();
    assert!(result.as_int().is_some());
    assert!(result.as_int().unwrap() > 0);
}

#[test]
fn test_debug_print_integration() {
    // debug-print should return the value
    assert_eq!(eval_source("(debug-print 42)").unwrap(), Value::int(42));
    assert_eq!(
        eval_source("(debug-print \"hello\")").unwrap(),
        Value::string("hello")
    );

    // Works with expressions
    assert_eq!(eval_source("(debug-print (+ 1 2))").unwrap(), Value::int(3));
}

#[test]
fn test_trace_integration() {
    // trace should return the second argument
    assert_eq!(eval_source("(trace \"label\" 42)").unwrap(), Value::int(42));
    assert_eq!(
        eval_source("(trace \"computation\" (+ 5 3))").unwrap(),
        Value::int(8)
    );
}

#[test]
fn test_memory_usage_integration() {
    // memory-usage should return a list
    let result = eval_source("(memory-usage)").unwrap();
    if result.is_cons() || result.is_nil() {
        // Valid list form
    } else {
        panic!("memory-usage should return a list");
    }
}

#[test]
fn test_concurrency_with_arithmetic() {
    // current-thread-id returns an integer, arithmetic with strings errors
    assert!(eval_source("(+ (current-thread-id) 1)").is_ok());
}

#[test]
fn test_debug_with_list_operations() {
    // Debug-print works in list operation chains
    assert_eq!(
        eval_source("(debug-print (list 1 2 3))").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_trace_with_arithmetic_chain() {
    // Multiple traces in computation
    let result = eval_source("(trace \"step1\" (+ 1 2))").unwrap();
    assert_eq!(result, Value::int(3));

    let result2 = eval_source("(trace \"step2\" (* 3 4))").unwrap();
    assert_eq!(result2, Value::int(12));
}

#[test]
fn test_multiple_debug_calls() {
    // Multiple debug-prints should work with begin
    assert_eq!(
        eval_source("(begin (debug-print 1) (debug-print 2) (debug-print 3))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_module_and_arithmetic_combination() {
    // Module primitives don't break normal arithmetic
    assert_eq!(eval_source("(+ 1 2)").unwrap(), Value::int(3));
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
    assert_eq!(eval_source("(+ 1 2)").unwrap(), Value::int(3));
}

#[test]
fn test_thread_id_consistency() {
    // Multiple calls should return same thread ID
    let id1 = eval_source("(current-thread-id)").unwrap();
    let id2 = eval_source("(current-thread-id)").unwrap();
    assert_eq!(id1, id2);
}

#[test]
fn test_debug_print_with_nested_structures() {
    // debug-print with nested lists
    assert!(eval_source("(debug-print (list (list 1 2) (list 3 4)))").is_ok());

    // debug-print with arrays
    assert!(eval_source("(debug-print (array 1 2 3))").is_ok());
}

#[test]
fn test_phase5_feature_availability() {
    // Verify all Phase 5 primitives are registered
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
    // spawn now requires a closure, not a native function
    assert!(eval_source("(spawn (fn () 42))").is_ok());
    // join requires a thread handle, not a string
    assert!(eval_source("(join (spawn (fn () 42)))").is_ok());
    assert!(eval_source("(time/sleep 0)").is_ok());
    assert!(eval_source("(current-thread-id)").is_ok());
    assert!(eval_source("(debug-print 42)").is_ok());
    assert!(eval_source("(trace \"x\" 42)").is_ok());
    assert!(eval_source("(memory-usage)").is_ok());
}

// Error cases for Phase 5 features

#[test]
fn test_import_file_wrong_argument_count() {
    // import-file requires exactly 1 argument
    assert!(eval_source("(import-file)").is_err());
    assert!(eval_source("(import-file \"a\" \"b\")").is_err());
}

#[test]
fn test_import_file_wrong_argument_type() {
    // import-file requires a string argument
    assert!(eval_source("(import-file 42)").is_err());
    assert!(eval_source("(import-file nil)").is_err());
}

#[test]
fn test_spawn_wrong_argument_count() {
    // spawn requires exactly 1 argument
    assert!(eval_source("(spawn)").is_err());
    assert!(eval_source("(spawn + *)").is_err());
}

#[test]
fn test_spawn_wrong_argument_type() {
    // spawn requires a function
    assert!(eval_source("(spawn 42)").is_err());
    assert!(eval_source("(spawn \"not a function\")").is_err());
}

#[test]
fn test_join_wrong_argument_count() {
    // join requires exactly 1 argument
    assert!(eval_source("(join)").is_err());
    assert!(eval_source("(join \"a\" \"b\")").is_err());
}

#[test]
fn test_sleep_wrong_argument_count() {
    // time/sleep requires exactly 1 argument
    assert!(eval_source("(time/sleep)").is_err());
    assert!(eval_source("(time/sleep 1 2)").is_err());
}

#[test]
fn test_sleep_wrong_argument_type() {
    // time/sleep requires a number
    assert!(eval_source("(time/sleep \"not a number\")").is_err());
    assert!(eval_source("(time/sleep nil)").is_err());
}

#[test]
fn test_sleep_negative_duration() {
    // time/sleep with negative duration should fail
    assert!(eval_source("(time/sleep -1)").is_err());
    assert!(eval_source("(time/sleep -0.5)").is_err());
}

#[test]
fn test_current_thread_id_no_arguments() {
    // current-thread-id takes no arguments
    assert!(eval_source("(current-thread-id)").is_ok());
}

#[test]
fn test_debug_print_wrong_argument_count() {
    // debug-print requires exactly 1 argument
    assert!(eval_source("(debug-print)").is_err());
    assert!(eval_source("(debug-print 1 2)").is_err());
}

#[test]
fn test_trace_wrong_argument_count() {
    // trace requires exactly 2 arguments
    assert!(eval_source("(trace)").is_err());
    assert!(eval_source("(trace \"label\")").is_err());
    assert!(eval_source("(trace \"a\" \"b\" \"c\")").is_err());
}

#[test]
fn test_trace_invalid_label_type() {
    // trace label must be string or symbol
    assert!(eval_source("(trace 42 100)").is_err());
    assert!(eval_source("(trace nil 100)").is_err());
}

#[test]
fn test_memory_usage_no_arguments() {
    // memory-usage takes no arguments
    assert!(eval_source("(memory-usage)").is_ok());
}

// Pattern matching tests

#[test]
fn test_match_syntax_parsing() {
    // Test that match syntax is properly parsed (not treated as function call)
    // Match expression should evaluate without errors
    assert!(eval_source("(match 5 (5 \"five\") (_ nil))").is_ok());
}

#[test]
fn test_match_wildcard_catches_any() {
    // Wildcard pattern matches any value
    assert!(eval_source("(match 42 (_ \"matched\"))").is_ok());
    assert!(eval_source("(match \"test\" (_ true))").is_ok());
}

#[test]
fn test_match_returns_result_expression() {
    // Match should return the value of the matched branch
    // Using literals to avoid variable binding complexity
    match eval_source("(match 5 (5 42) (10 0) (_ nil))") {
        Ok(v) => {
            if let Some(n) = v.as_int() {
                assert!(n > 0, "Should return a positive number");
            } else {
                panic!("Expected Int, got {:?}", v);
            }
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }
}

#[test]
fn test_match_clause_ordering() {
    // First matching clause should be used
    assert!(eval_source("(match 5 (5 true) (5 false) (_ nil))").is_ok());
}

#[test]
fn test_match_default_wildcard() {
    // Wildcard pattern should match when no literals match
    assert!(eval_source("(match 99 (1 \"one\") (2 \"two\") (_ \"other\"))").is_ok());
}

#[test]
fn test_match_nil_pattern_parsing() {
    // Nil pattern should parse and work
    assert!(eval_source("(match nil (nil \"empty\") (_ nil))").is_ok());
}

#[test]
fn test_match_wildcard_pattern() {
    // Match with wildcard (_) - catches any value
    assert_eq!(
        eval_source("(match 42 (_ \"any\"))").unwrap(),
        Value::string("any")
    );
    assert_eq!(
        eval_source("(match \"hello\" (_ \"matched\"))").unwrap(),
        Value::string("matched")
    );
}

#[test]
fn test_match_nil_pattern() {
    // Match nil
    assert_eq!(
        eval_source("(match nil (nil \"empty\") (_ nil))").unwrap(),
        Value::string("empty")
    );
    // nil pattern should NOT match empty list
    assert_eq!(
        eval_source("(match (list) (nil \"empty\") (_ \"not-nil\"))").unwrap(),
        Value::string("not-nil")
    );
}

#[test]
fn test_match_default_case() {
    // Default pattern at end - catches anything not matched
    assert_eq!(
        eval_source("(match 99 (1 \"one\") (2 \"two\") (_ \"other\"))").unwrap(),
        Value::string("other")
    );
}

#[test]
fn test_match_multiple_clauses_ordering() {
    // Test clause ordering - first matching clause wins
    assert_eq!(
        eval_source("(match 2 (1 \"one\") (2 \"two\") (3 \"three\") (_ nil))").unwrap(),
        Value::string("two")
    );
    assert_eq!(
        eval_source("(match 1 (1 \"one\") (2 \"two\") (3 \"three\") (_ nil))").unwrap(),
        Value::string("one")
    );
}

#[test]
fn test_match_with_static_expressions() {
    // Matched expressions should be evaluated (without pattern variable binding)
    assert_eq!(
        eval_source("(match 10 (10 (* 2 3)) (_ nil))").unwrap(),
        Value::int(6)
    );
    assert_eq!(
        eval_source("(match 5 (5 (+ 1 1)) (_ nil))").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_match_string_literals() {
    // Match string literals
    assert_eq!(
        eval_source("(match \"hello\" (\"hello\" \"matched\") (_ \"no\"))").unwrap(),
        Value::string("matched")
    );
}

// Integration scenarios
#[test]
fn test_error_in_trace_argument() {
    // trace should still work even if computation had errors
    assert!(eval_source("(trace \"bad\" (undefined-var))").is_err());
}

#[test]
fn test_debug_and_trace_chain() {
    // Both can be used together
    assert!(eval_source("(trace \"a\" (debug-print (+ 1 2)))").is_ok());
}

#[test]
fn test_sleep_in_arithmetic_context() {
    // Sleep returns nil which can't be used in arithmetic
    assert!(eval_source("(+ 1 (time/sleep 0))").is_err());
}

#[test]
fn test_import_file_returns_last_value() {
    // import-file returns the last expression's value from the loaded file.
    // test.lisp ends with a closure that returns a struct of exports.
    // Call the closure to get the exports struct, then use get for field access.
    let result = eval_source(
        "(def exports ((import-file \"test-modules/test.lisp\")))
         (get exports :test-var)",
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_import_file_with_function_definitions() {
    // Load a file that defines functions
    // Note: This test skipped because math-lib.elle uses recursion which requires proper module context
    // Uncomment when module context is fully implemented
    // assert!(eval_source("(import-file \"test-modules/math-lib.lisp\")").is_ok());
}

#[test]
fn test_import_file_with_variable_definitions() {
    // Load a file that defines variables
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
}

#[test]
fn test_import_multiple_files_sequentially() {
    // Load multiple files in sequence
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
    // Only load files with simple definitions to avoid recursion issues
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
}

#[test]
fn test_import_same_file_twice_idempotent() {
    // Within a single VM, loading the same file twice is idempotent:
    // first load returns the module closure, second returns true (already loaded)
    let result = eval_source(
        "(def r1 (import-file \"test-modules/test.lisp\"))
         (def r2 (import-file \"test-modules/test.lisp\"))
         (list (fn? r1) (= r2 true))",
    );
    assert!(result.is_ok());
    // r1 is a closure (module export function), r2 is true (already loaded)
    assert_eq!(
        result.unwrap(),
        elle::list([Value::bool(true), Value::bool(true)])
    );
}

#[test]
fn test_import_file_with_relative_paths() {
    // Test various relative path formats
    assert!(eval_source("(import-file \"./test-modules/test.lisp\")").is_ok());
    // Only test with simple files to avoid recursion issues
    assert!(eval_source("(import-file \"test-modules/test.lisp\")").is_ok());
}

// Array pattern matching tests

#[test]
fn test_match_array_literal() {
    assert_eq!(
        eval_source("(match [1 2 3] ([1 2 3] \"exact\") (_ \"no\"))").unwrap(),
        Value::string("exact")
    );
}

#[test]
fn test_match_array_binding() {
    assert_eq!(
        eval_source("(match [10 20] ([a b] (+ a b)) (_ 0))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_match_array_wrong_length() {
    assert_eq!(
        eval_source("(match [1 2] ([a b c] \"three\") ([a b] \"two\") (_ nil))").unwrap(),
        Value::string("two")
    );
}

#[test]
fn test_match_array_not_array() {
    assert_eq!(
        eval_source("(match 42 ([a b] \"array\") (_ \"other\"))").unwrap(),
        Value::string("other")
    );
}

#[test]
fn test_match_array_empty() {
    assert_eq!(
        eval_source("(match [] ([] \"empty\") (_ \"other\"))").unwrap(),
        Value::string("empty")
    );
}

#[test]
fn test_match_array_rest() {
    // & rest captures remaining elements
    assert_eq!(
        eval_source("(match [1 2 3 4] ([a & rest] (length rest)) (_ 0))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_match_array_nested() {
    assert_eq!(
        eval_source("(match [1 [2 3]] ([a [b c]] (+ a (+ b c))) (_ 0))").unwrap(),
        Value::int(6)
    );
}

// Guard (when) tests

#[test]
fn test_match_guard_basic() {
    assert_eq!(
        eval_source("(match 5 (x when (> x 3) \"big\") (x \"small\"))").unwrap(),
        Value::string("big")
    );
    assert_eq!(
        eval_source("(match 2 (x when (> x 3) \"big\") (x \"small\"))").unwrap(),
        Value::string("small")
    );
}

#[test]
fn test_match_guard_with_literal() {
    assert_eq!(
        eval_source("(match 10 (10 when false \"nope\") (10 \"yes\") (_ nil))").unwrap(),
        Value::string("yes")
    );
}

// Cons pattern tests

#[test]
fn test_match_cons_pattern() {
    assert_eq!(
        eval_source("(match (cons 1 2) ((h . t) (+ h t)) (_ 0))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_match_cons_not_pair() {
    assert_eq!(
        eval_source("(match 42 ((h . t) \"pair\") (_ \"nope\"))").unwrap(),
        Value::string("nope")
    );
}

// List rest pattern tests

#[test]
fn test_match_list_rest() {
    assert_eq!(
        eval_source("(match (list 1 2 3) ((a & rest) a) (_ nil))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_match_list_exact_length() {
    // List pattern without rest must match exact length
    assert_eq!(
        eval_source("(match (list 1 2 3) ((1 2) \"two\") ((1 2 3) \"three\") (_ nil))").unwrap(),
        Value::string("three")
    );
}

// Keyword pattern test

#[test]
fn test_match_keyword_literal() {
    assert_eq!(
        eval_source("(match :foo (:foo \"matched\") (_ \"no\"))").unwrap(),
        Value::string("matched")
    );
    assert_eq!(
        eval_source("(match :bar (:foo \"matched\") (_ \"no\"))").unwrap(),
        Value::string("no")
    );
}

// Variable binding test

#[test]
fn test_match_variable_binding() {
    assert_eq!(
        eval_source("(match 42 (x (+ x 1)))").unwrap(),
        Value::int(43)
    );
}

// Non-exhaustive match is a compile-time error

#[test]
fn test_match_non_exhaustive_is_error() {
    let result = eval_source("(match 42 (1 \"one\") (2 \"two\"))");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("non-exhaustive"),
        "error should mention non-exhaustive"
    );
}

// Variadic macro tests

#[test]
fn test_variadic_macro_basic() {
    assert_eq!(
        eval_source("(begin (defmacro my-list (& items) `(list ,;items)) (my-list 1 2 3))")
            .unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_macro_fixed_and_rest() {
    assert_eq!(
        eval_source("(begin (defmacro my-add (first & rest) `(+ ,first ,;rest)) (my-add 1 2 3))")
            .unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_variadic_macro_empty_rest() {
    assert_eq!(
        eval_source("(begin (defmacro my-list (& items) `(list ,;items)) (my-list))").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_variadic_macro_arity_error() {
    assert!(
        eval_source("(begin (defmacro foo (a b & rest) `(list ,a ,b ,;rest)) (foo 1))").is_err()
    );
}

#[test]
fn test_variadic_macro_when_multi_body() {
    // when with multiple body expressions via & rest
    assert_eq!(
        eval_source("(begin (defmacro my-when (test & body) `(if ,test (begin ,;body) nil)) (my-when true 1 2 3))")
            .unwrap(),
        Value::int(3)
    );
}

// === match: improper list patterns (a b . c) ===

#[test]
fn test_match_improper_list_pattern() {
    // (a b . c) should match a list of 2+ elements, binding the rest to c
    let result =
        eval_source("(match (cons 1 (cons 2 3)) ((a b . c) (list a b c)) (_ :no))").unwrap();
    // a=1, b=2, c=3
    assert_eq!(result.to_string(), "(1 2 3)");
}

#[test]
fn test_match_improper_list_pattern_longer() {
    // (a b c . d) should match a list of 3+ elements
    let result =
        eval_source("(match (list 1 2 3 4 5) ((a b c . d) (list a b c d)) (_ :no))").unwrap();
    // a=1, b=2, c=3, d=(4 5)
    assert_eq!(result.to_string(), "(1 2 3 (4 5))");
}

#[test]
fn test_match_improper_list_pattern_exact() {
    // When the value has exactly the right number of elements for the dot pattern
    let result = eval_source("(match (cons 1 2) ((a . b) (list a b)) (_ :no))").unwrap();
    // This already works — regression guard
    assert_eq!(result.to_string(), "(1 2)");
}

#[test]
fn test_match_improper_list_pattern_too_short() {
    // Value too short for the pattern — should fall through
    let result = eval_source("(match (list 1) ((a b . c) :matched) (_ :no))").unwrap();
    assert_eq!(result, Value::keyword("no"));
}

// === match: or-patterns (1 | 2 | 3) ===

#[test]
fn test_or_pattern_basic() {
    assert_eq!(
        eval_source("(match 2 ((1 | 2 | 3) :small) (_ :big))").unwrap(),
        Value::keyword("small")
    );
}

#[test]
fn test_or_pattern_no_match() {
    assert_eq!(
        eval_source("(match 5 ((1 | 2 | 3) :small) (_ :big))").unwrap(),
        Value::keyword("big")
    );
}

#[test]
fn test_or_pattern_keywords() {
    assert_eq!(
        eval_source("(match :b ((:a | :b | :c) :found) (_ :not))").unwrap(),
        Value::keyword("found")
    );
}

#[test]
fn test_or_pattern_with_binding() {
    assert_eq!(
        eval_source("(match (cons 1 2) (((x . _) | (_ . x)) x) (_ 0))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_or_pattern_with_binding_second() {
    assert_eq!(
        eval_source("(match 99 (((x . _) | x) x) (_ 0))").unwrap(),
        Value::int(99)
    );
}

#[test]
fn test_or_pattern_different_bindings_error() {
    let result = eval_source("(match 1 (((x . y) | (x . _)) :ok) (_ :no))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("different variables"));
}

#[test]
fn test_or_pattern_with_guard() {
    assert_eq!(
        eval_source("(match 2 ((1 | 2 | 3) when true :yes) (_ :no))").unwrap(),
        Value::keyword("yes")
    );
}

#[test]
fn test_or_pattern_nested_in_cons() {
    assert_eq!(
        eval_source("(match (cons 2 :x) (((1 | 2) . t) t) (_ :fail))").unwrap(),
        Value::keyword("x")
    );
}

#[test]
fn test_or_pattern_multi_item_error() {
    let result = eval_source("(match 1 ((a b | c d) :ok) (_ :no))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("single pattern"));
}

#[test]
fn test_or_pattern_two_alternatives() {
    assert_eq!(
        eval_source("(match :y ((:x | :y) :found) (_ :not))").unwrap(),
        Value::keyword("found")
    );
}

#[test]
fn test_or_pattern_with_nil() {
    assert_eq!(
        eval_source("(match nil ((nil | 0) :empty) (_ :other))").unwrap(),
        Value::keyword("empty")
    );
}

#[test]
fn test_or_pattern_in_tuple() {
    assert_eq!(
        eval_source("(match [2 :x] ([(1 | 2) y] y) (_ :fail))").unwrap(),
        Value::keyword("x")
    );
}

// =========================================================================
// Guard test coverage (Chunk 4)
// =========================================================================

#[test]
fn test_guard_references_pattern_var() {
    assert_eq!(
        eval_source("(match 10 (x when (> x 5) :big) (x :small))").unwrap(),
        Value::keyword("big")
    );
    assert_eq!(
        eval_source("(match 3 (x when (> x 5) :big) (x :small))").unwrap(),
        Value::keyword("small")
    );
}

#[test]
fn test_guard_fallthrough() {
    assert_eq!(
        eval_source("(match 5 (x when false :never) (x :always))").unwrap(),
        Value::keyword("always")
    );
}

#[test]
fn test_guard_with_cons() {
    assert_eq!(
        eval_source("(match (cons 1 2) ((h . t) when (> h 0) (+ h t)) (_ 0))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_guard_with_list() {
    assert_eq!(
        eval_source("(match (list 1 2 3) ((a b c) when (> (+ a b c) 5) :big) (_ :small))").unwrap(),
        Value::keyword("big")
    );
}

#[test]
fn test_guard_with_tuple() {
    assert_eq!(
        eval_source("(match [1 2] ([a b] when (< a b) :ordered) (_ :no))").unwrap(),
        Value::keyword("ordered")
    );
}

#[test]
fn test_guard_with_struct() {
    assert_eq!(
        eval_source("(match {:x 10 :y 20} ({:x x :y y} when (> y x) :valid) (_ :no))").unwrap(),
        Value::keyword("valid")
    );
}

#[test]
fn test_guard_with_rest() {
    assert_eq!(
        eval_source("(match (list 1 2 3) ((a & rest) when (> a 0) rest) (_ :fail))").unwrap(),
        eval_source("(list 2 3)").unwrap()
    );
}

#[test]
fn test_guard_fallthrough_to_wildcard() {
    assert_eq!(
        eval_source("(match 5 (x when false :a) (_ :fallback))").unwrap(),
        Value::keyword("fallback")
    );
}

#[test]
fn test_guard_complex_body() {
    assert_eq!(
        eval_source("(match 10 (x when (> x 5) (let ((y (* x 2))) y)) (x x))").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_guard_no_binding_leak() {
    assert_eq!(
        eval_source("(match 5 (x when false x) (y (+ y 1)))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_guard_middle_arm_matches() {
    assert_eq!(
        eval_source("(match 5 (x when (> x 10) :big) (x when (> x 3) :medium) (x :small))")
            .unwrap(),
        Value::keyword("medium")
    );
}

#[test]
fn test_or_pattern_guard_outer_var() {
    assert_eq!(
        eval_source(
            "(let ((threshold 3)) (match 2 ((1 | 2 | 3) when (< threshold 5) :yes) (_ :no)))"
        )
        .unwrap(),
        Value::keyword("yes")
    );
}

#[test]
fn test_or_pattern_binding_guard() {
    assert_eq!(
        eval_source("(match (cons 6 :x) (((a . _) | (_ . a)) when (> a 5) :big) (_ :small))")
            .unwrap(),
        Value::keyword("big")
    );
}

#[test]
fn test_or_pattern_guard_fallthrough() {
    assert_eq!(
        eval_source("(match 2 ((1 | 2 | 3) when false :never) (_ :fallback))").unwrap(),
        Value::keyword("fallback")
    );
}

// === Exhaustiveness tests ===

#[test]
fn test_exhaustive_match_with_wildcard() {
    assert_eq!(
        eval_source("(match 42 (1 :one) (_ :other))").unwrap(),
        Value::keyword("other")
    );
}

#[test]
fn test_exhaustive_match_with_variable() {
    assert_eq!(
        eval_source("(match 42 (1 :one) (x x))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_non_exhaustive_match_error() {
    let result = eval_source("(match 42 (1 :one) (2 :two))");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("non-exhaustive"),
        "error should mention non-exhaustive"
    );
}

#[test]
fn test_exhaustive_match_booleans() {
    assert_eq!(
        eval_source("(match true (true :t) (false :f))").unwrap(),
        Value::keyword("t")
    );
}

#[test]
fn test_exhaustive_or_pattern_booleans() {
    assert_eq!(
        eval_source("(match true ((true | false) :both))").unwrap(),
        Value::keyword("both")
    );
}

#[test]
fn test_non_exhaustive_guard_on_last_arm() {
    // A wildcard with a guard is NOT exhaustive
    let result = eval_source("(match 42 (x when (> x 0) :pos))");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("non-exhaustive"),
        "error should mention non-exhaustive"
    );
}

// === Decision tree specific tests ===

#[test]
fn test_decision_tree_shared_prefix() {
    // Two arms share a cons prefix — decision tree should check cons once
    let result = eval_source(
        "(match (list 1 2 3)
           ((1 2 3) :exact)
           ((1 2 _) :prefix)
           (_ :other))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("exact"));
}

#[test]
fn test_decision_tree_shared_prefix_second_arm() {
    let result = eval_source(
        "(match (list 1 2 4)
           ((1 2 3) :exact)
           ((1 2 _) :prefix)
           (_ :other))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("prefix"));
}

#[test]
fn test_decision_tree_multiple_constructors() {
    // Different constructors in the same column
    let result = eval_source(
        "(match (list 1 2)
           (nil :nil)
           ((h . t) :pair)
           (_ :other))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("pair"));
}

#[test]
fn test_decision_tree_literal_discrimination() {
    // Multiple literal arms — decision tree switches on value
    let result = eval_source(
        "(match :c
           (:a 1)
           (:b 2)
           (:c 3)
           (:d 4)
           (_ 0))",
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn test_decision_tree_nested_tuple_match() {
    let result = eval_source(
        "(match [1 [2 3]]
           ([1 [2 3]] :exact)
           ([1 [2 _]] :partial)
           ([_ _] :any-pair)
           (_ :other))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("exact"));
}

#[test]
fn test_decision_tree_guard_fallthrough_to_next_constructor() {
    // Guard fails, should try next arm even with different constructor
    let result = eval_source(
        "(match 5
           (5 when false :guarded)
           (5 :unguarded)
           (_ :default))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("unguarded"));
}

#[test]
fn test_decision_tree_or_pattern_with_shared_body() {
    let result = eval_source(
        "(match :b
           ((:a | :b | :c) :first-group)
           ((:d | :e | :f) :second-group)
           (_ :other))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("first-group"));
}

#[test]
fn test_decision_tree_struct_key_discrimination() {
    // Two struct patterns with overlapping keys
    let result = eval_source(
        "(match {:type :circle :radius 5}
           ({:type :circle :radius r} r)
           ({:type :square :side s} s)
           (_ 0))",
    )
    .unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn test_decision_tree_struct_key_discrimination_second() {
    let result = eval_source(
        "(match {:type :square :side 7}
           ({:type :circle :radius r} r)
           ({:type :square :side s} s)
           (_ 0))",
    )
    .unwrap();
    assert_eq!(result, Value::int(7));
}

#[test]
fn test_decision_tree_deeply_nested() {
    let result = eval_source(
        "(match (list 1 (list 2 (list 3)))
           ((1 (2 (3))) :deep)
           ((1 (2 _)) :medium)
           ((1 _) :shallow)
           (_ :none))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("deep"));
}

#[test]
fn test_decision_tree_match_in_loop() {
    // Match inside a loop — exercises repeated decision tree execution
    let result = eval_source(
        "(var result (list))
         (each i (list 1 2 3)
           (set result (cons (match i
                               (1 :one)
                               (2 :two)
                               (3 :three)
                               (_ :other))
                             result)))
         (reverse result)",
    )
    .unwrap();
    assert_eq!(result, eval_source("(list :one :two :three)").unwrap());
}

#[test]
fn test_decision_tree_boolean_exhaustive() {
    // Boolean exhaustiveness — no wildcard needed
    let result = eval_source(
        "(match false
           (true :yes)
           (false :no))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("no"));
}

#[test]
fn test_decision_tree_or_boolean_exhaustive() {
    let result = eval_source(
        "(match true
           ((true | false) :bool))",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("bool"));
}

#[test]
fn test_or_pattern_decision_tree_shared() {
    assert_eq!(
        eval_source("(match (cons 1 :x) ((1 . t) t) ((2 . t) t) (((3 | 4) . t) t) (_ :fail))",)
            .unwrap(),
        Value::keyword("x")
    );
}

#[test]
fn test_or_pattern_nested_decision_tree() {
    assert_eq!(
        eval_source("(match [3 :y] ([(1 | 2 | 3) v] v) (_ :fail))").unwrap(),
        Value::keyword("y")
    );
}

#[test]
fn test_or_pattern_guard_decision_tree() {
    assert_eq!(
        eval_source("(match 5 ((1 | 2 | 3) when true :small) ((4 | 5 | 6) :medium) (_ :big))",)
            .unwrap(),
        Value::keyword("medium")
    );
}
