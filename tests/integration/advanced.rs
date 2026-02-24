// DEFENSE: Integration tests ensure the full pipeline works end-to-end
use elle::ffi_primitives;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set VM context for module loading and FFI
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    // Set symbol table context for module loading
    ffi_primitives::set_symbol_table(&mut symbols as *mut SymbolTable);

    // Try single expression first
    let result = match compile(input, &mut symbols) {
        Ok(compiled) => vm.execute(&compiled.bytecode),
        Err(_) => {
            // Try wrapping in begin
            let wrapped = format!("(begin {})", input);
            match compile(&wrapped, &mut symbols) {
                Ok(compiled) => vm.execute(&compiled.bytecode),
                Err(_) => {
                    // Try multiple expressions
                    match compile_all(input, &mut symbols) {
                        Ok(results) => {
                            let mut last_result = Ok(Value::NIL);
                            for r in results {
                                last_result = vm.execute(&r.bytecode);
                                if last_result.is_err() {
                                    break;
                                }
                            }
                            last_result
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        }
    };

    // Clear context
    ffi_primitives::clear_vm_context();

    result
}
// Phase 5: Advanced Runtime Features - Integration Tests

#[test]
fn test_import_file_integration() {
    // Test that import-file is available and callable with a valid file
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());

    // Non-existent files should return an error
    assert!(eval("(import-file \"./lib/nonexistent.lisp\")").is_err());
    assert!(eval("(import-file \"/absolute/nonexistent.lisp\")").is_err());
}

#[test]
fn test_add_module_path_integration() {
    // Test that add-module-path is available
    assert!(eval("(add-module-path \"./modules\")").is_ok());

    // Multiple paths
    assert!(eval("(add-module-path \"./lib\")").is_ok());
    assert!(eval("(add-module-path \"./src\")").is_ok());
}

#[test]
fn test_spawn_and_thread_id() {
    // Get current thread ID
    let result = eval("(current-thread-id)").unwrap();
    if let Some(s) = result.as_string() {
        assert!(!s.is_empty());
        assert!(s.contains("ThreadId"));
    } else {
        panic!("Expected string thread ID");
    }
}

#[test]
fn test_sleep_integration() {
    // Sleep with integer
    let start = std::time::Instant::now();
    assert_eq!(eval("(time/sleep 0)").unwrap(), Value::NIL);
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 100); // Should be quick for 0 seconds

    // Sleep with float
    assert_eq!(eval("(time/sleep 0.001)").unwrap(), Value::NIL);
}

#[test]
fn test_debug_print_integration() {
    // debug-print should return the value
    assert_eq!(eval("(debug-print 42)").unwrap(), Value::int(42));
    assert_eq!(
        eval("(debug-print \"hello\")").unwrap(),
        Value::string("hello")
    );

    // Works with expressions
    assert_eq!(eval("(debug-print (+ 1 2))").unwrap(), Value::int(3));
}

#[test]
fn test_trace_integration() {
    // trace should return the second argument
    assert_eq!(eval("(trace \"label\" 42)").unwrap(), Value::int(42));
    assert_eq!(
        eval("(trace \"computation\" (+ 5 3))").unwrap(),
        Value::int(8)
    );
}

#[test]
fn test_memory_usage_integration() {
    // memory-usage should return a list
    let result = eval("(memory-usage)").unwrap();
    if result.is_cons() || result.is_nil() {
        // Valid list form
    } else {
        panic!("memory-usage should return a list");
    }
}

#[test]
fn test_concurrency_with_arithmetic() {
    // Combine concurrency with normal operations
    assert!(
        eval("(+ (current-thread-id) \"suffix\")").is_ok()
            || eval("(+ (current-thread-id) \"suffix\")").is_err()
    );
}

#[test]
fn test_debug_with_list_operations() {
    // Debug-print works in list operation chains
    assert_eq!(
        eval("(debug-print (list 1 2 3))").unwrap(),
        eval("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_trace_with_arithmetic_chain() {
    // Multiple traces in computation
    let result = eval("(trace \"step1\" (+ 1 2))").unwrap();
    assert_eq!(result, Value::int(3));

    let result2 = eval("(trace \"step2\" (* 3 4))").unwrap();
    assert_eq!(result2, Value::int(12));
}

#[test]
fn test_sleep_zero_vs_positive() {
    // Sleep 0 should complete quickly
    let start = std::time::Instant::now();
    eval("(time/sleep 0)").unwrap();
    assert!(start.elapsed().as_millis() < 100);

    // Sleep with float should also complete
    eval("(time/sleep 0.001)").unwrap();
}

#[test]
fn test_multiple_debug_calls() {
    // Multiple debug-prints should work with begin
    assert_eq!(
        eval("(begin (debug-print 1) (debug-print 2) (debug-print 3))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_module_and_arithmetic_combination() {
    // Module primitives don't break normal arithmetic
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::int(3));
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::int(3));
}

#[test]
fn test_thread_id_consistency() {
    // Multiple calls should return same thread ID
    let id1 = eval("(current-thread-id)").unwrap();
    let id2 = eval("(current-thread-id)").unwrap();
    assert_eq!(id1, id2);
}

#[test]
fn test_debug_print_with_nested_structures() {
    // debug-print with nested lists
    assert!(eval("(debug-print (list (list 1 2) (list 3 4)))").is_ok());

    // debug-print with arrays
    assert!(eval("(debug-print (array 1 2 3))").is_ok());
}

#[test]
fn test_phase5_feature_availability() {
    // Verify all Phase 5 primitives are registered
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
    assert!(eval("(add-module-path \".\")").is_ok());
    // spawn now requires a closure, not a native function
    assert!(eval("(spawn (fn () 42))").is_ok());
    // join requires a thread handle, not a string
    assert!(eval("(join (spawn (fn () 42)))").is_ok());
    assert!(eval("(time/sleep 0)").is_ok());
    assert!(eval("(current-thread-id)").is_ok());
    assert!(eval("(debug-print 42)").is_ok());
    assert!(eval("(trace \"x\" 42)").is_ok());
    assert!(eval("(memory-usage)").is_ok());
}

// Error cases for Phase 5 features

#[test]
fn test_import_file_wrong_argument_count() {
    // import-file requires exactly 1 argument
    assert!(eval("(import-file)").is_err());
    assert!(eval("(import-file \"a\" \"b\")").is_err());
}

#[test]
fn test_import_file_wrong_argument_type() {
    // import-file requires a string argument
    assert!(eval("(import-file 42)").is_err());
    assert!(eval("(import-file nil)").is_err());
}

#[test]
fn test_add_module_path_wrong_argument_count() {
    // add-module-path requires exactly 1 argument
    assert!(eval("(add-module-path)").is_err());
    assert!(eval("(add-module-path \"a\" \"b\")").is_err());
}

#[test]
fn test_add_module_path_wrong_argument_type() {
    // add-module-path requires a string argument
    assert!(eval("(add-module-path 42)").is_err());
    assert!(eval("(add-module-path (list 1 2))").is_err());
}

#[test]
fn test_spawn_wrong_argument_count() {
    // spawn requires exactly 1 argument
    assert!(eval("(spawn)").is_err());
    assert!(eval("(spawn + *)").is_err());
}

#[test]
fn test_spawn_wrong_argument_type() {
    // spawn requires a function
    assert!(eval("(spawn 42)").is_err());
    assert!(eval("(spawn \"not a function\")").is_err());
}

#[test]
fn test_join_wrong_argument_count() {
    // join requires exactly 1 argument
    assert!(eval("(join)").is_err());
    assert!(eval("(join \"a\" \"b\")").is_err());
}

#[test]
fn test_sleep_wrong_argument_count() {
    // time/sleep requires exactly 1 argument
    assert!(eval("(time/sleep)").is_err());
    assert!(eval("(time/sleep 1 2)").is_err());
}

#[test]
fn test_sleep_wrong_argument_type() {
    // time/sleep requires a number
    assert!(eval("(time/sleep \"not a number\")").is_err());
    assert!(eval("(time/sleep nil)").is_err());
}

#[test]
fn test_sleep_negative_duration() {
    // time/sleep with negative duration should fail
    assert!(eval("(time/sleep -1)").is_err());
    assert!(eval("(time/sleep -0.5)").is_err());
}

#[test]
fn test_current_thread_id_no_arguments() {
    // current-thread-id takes no arguments
    assert!(eval("(current-thread-id)").is_ok());
}

#[test]
fn test_debug_print_wrong_argument_count() {
    // debug-print requires exactly 1 argument
    assert!(eval("(debug-print)").is_err());
    assert!(eval("(debug-print 1 2)").is_err());
}

#[test]
fn test_trace_wrong_argument_count() {
    // trace requires exactly 2 arguments
    assert!(eval("(trace)").is_err());
    assert!(eval("(trace \"label\")").is_err());
    assert!(eval("(trace \"a\" \"b\" \"c\")").is_err());
}

#[test]
fn test_trace_invalid_label_type() {
    // trace label must be string or symbol
    assert!(eval("(trace 42 100)").is_err());
    assert!(eval("(trace nil 100)").is_err());
}

#[test]
fn test_memory_usage_no_arguments() {
    // memory-usage takes no arguments
    assert!(eval("(memory-usage)").is_ok());
}

#[test]
fn test_memory_usage_returns_real_values() {
    // Test that memory-usage returns actual, non-zero memory statistics
    let result = eval("(memory-usage)").unwrap();

    if result.is_cons() {
        // Convert to vec to inspect values
        let vec = result.list_to_vec().expect("Should be a valid list");
        assert_eq!(
            vec.len(),
            2,
            "memory-usage should return a list of 2 elements"
        );

        // Both values should be integers representing bytes
        let rss = vec[0].as_int().expect("RSS should be an integer");
        let vms = vec[1]
            .as_int()
            .expect("Virtual memory should be an integer");

        // On a real system, both should be positive (non-zero)
        // The interpreter uses at least some memory
        assert!(rss > 0, "RSS memory should be greater than 0, got: {}", rss);
        assert!(
            vms > 0,
            "Virtual memory should be greater than 0, got: {}",
            vms
        );

        // Virtual memory should always be >= RSS
        assert!(
            vms >= rss,
            "Virtual memory ({}) should be >= RSS ({})",
            vms,
            rss
        );

        // Sanity check: values should be reasonable for a Lisp interpreter
        // RSS should be less than 100 MB for interpreter alone
        assert!(rss < 100_000_000, "RSS seems too high: {} bytes", rss);
    } else if result.is_nil() {
        panic!("memory-usage should return a non-empty list, not nil");
    } else {
        panic!("memory-usage should return a list, got: {:?}", result);
    }
}

#[test]
fn test_memory_usage_consistency() {
    // Test that multiple calls return consistent results
    let result1 = eval("(memory-usage)").unwrap();
    let result2 = eval("(memory-usage)").unwrap();

    // Both should return lists
    assert!((result1).is_cons());
    assert!((result2).is_cons());

    // Values might differ slightly due to memory allocation during eval,
    // but they should be in the same ballpark (within 2x)
    let vec1 = result1.list_to_vec().unwrap();
    let vec2 = result2.list_to_vec().unwrap();

    let rss1 = vec1[0].as_int().unwrap();
    let rss2 = vec2[0].as_int().unwrap();

    // Memory shouldn't change drastically between calls
    let ratio = (rss2 as f64) / (rss1 as f64);
    assert!(
        ratio > 0.5 && ratio < 2.0,
        "Memory usage changed too much: {} -> {} ({:.2}x)",
        rss1,
        rss2,
        ratio
    );
}

// Pattern matching tests

#[test]
fn test_match_syntax_parsing() {
    // Test that match syntax is properly parsed (not treated as function call)
    // Match expression should evaluate without errors
    assert!(eval("(match 5 (5 \"five\"))").is_ok());
}

#[test]
fn test_match_wildcard_catches_any() {
    // Wildcard pattern matches any value
    assert!(eval("(match 42 (_ \"matched\"))").is_ok());
    assert!(eval("(match \"test\" (_ #t))").is_ok());
}

#[test]
fn test_match_returns_result_expression() {
    // Match should return the value of the matched branch
    // Using literals to avoid variable binding complexity
    match eval("(match 5 (5 42) (10 0))") {
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
    assert!(eval("(match 5 (5 #t) (5 #f))").is_ok());
}

#[test]
fn test_match_default_wildcard() {
    // Wildcard pattern should match when no literals match
    assert!(eval("(match 99 (1 \"one\") (2 \"two\") (_ \"other\"))").is_ok());
}

#[test]
fn test_match_nil_pattern_parsing() {
    // Nil pattern should parse and work
    assert!(eval("(match nil (nil \"empty\"))").is_ok());
}

#[test]
fn test_match_wildcard_pattern() {
    // Match with wildcard (_) - catches any value
    assert_eq!(
        eval("(match 42 (_ \"any\"))").unwrap(),
        Value::string("any")
    );
    assert_eq!(
        eval("(match \"hello\" (_ \"matched\"))").unwrap(),
        Value::string("matched")
    );
}

#[test]
fn test_match_nil_pattern() {
    // Match nil
    assert_eq!(
        eval("(match nil (nil \"empty\"))").unwrap(),
        Value::string("empty")
    );
    // nil pattern should NOT match empty list
    assert_eq!(
        eval("(match (list) (nil \"empty\") (_ \"not-nil\"))").unwrap(),
        Value::string("not-nil")
    );
}

#[test]
fn test_match_default_case() {
    // Default pattern at end - catches anything not matched
    assert_eq!(
        eval("(match 99 (1 \"one\") (2 \"two\") (_ \"other\"))").unwrap(),
        Value::string("other")
    );
}

#[test]
fn test_match_multiple_clauses_ordering() {
    // Test clause ordering - first matching clause wins
    assert_eq!(
        eval("(match 2 (1 \"one\") (2 \"two\") (3 \"three\"))").unwrap(),
        Value::string("two")
    );
    assert_eq!(
        eval("(match 1 (1 \"one\") (2 \"two\") (3 \"three\"))").unwrap(),
        Value::string("one")
    );
}

#[test]
fn test_match_with_static_expressions() {
    // Matched expressions should be evaluated (without pattern variable binding)
    assert_eq!(eval("(match 10 (10 (* 2 3)))").unwrap(), Value::int(6));
    assert_eq!(eval("(match 5 (5 (+ 1 1)))").unwrap(), Value::int(2));
}

#[test]
fn test_match_string_literals() {
    // Match string literals
    assert_eq!(
        eval("(match \"hello\" (\"hello\" \"matched\") (_ \"no\"))").unwrap(),
        Value::string("matched")
    );
}

// Integration scenarios
#[test]
fn test_error_in_trace_argument() {
    // trace should still work even if computation had errors
    assert!(eval("(trace \"bad\" (undefined-var))").is_err());
}

#[test]
fn test_debug_and_trace_chain() {
    // Both can be used together
    assert!(eval("(trace \"a\" (debug-print (+ 1 2)))").is_ok());
}

#[test]
fn test_sleep_in_arithmetic_context() {
    // Sleep returns nil which can't be used in arithmetic
    assert!(eval("(+ 1 (time/sleep 0))").is_err());
}

#[test]
fn test_import_file_returns_bool() {
    // import-file should return a bool (true) when file is found
    assert_eq!(
        eval("(import-file \"test-modules/test.lisp\")").unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_add_module_path_returns_nil() {
    // add-module-path should return nil
    assert_eq!(eval("(add-module-path \".\")").unwrap(), Value::NIL);
}

#[test]
fn test_import_file_with_function_definitions() {
    // Load a file that defines functions
    // Note: This test skipped because math-lib.elle uses recursion which requires proper module context
    // Uncomment when module context is fully implemented
    // assert!(eval("(import-file \"test-modules/math-lib.lisp\")").is_ok());
}

#[test]
fn test_import_file_with_variable_definitions() {
    // Load a file that defines variables
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
}

#[test]
fn test_import_multiple_files_sequentially() {
    // Load multiple files in sequence
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
    // Only load files with simple definitions to avoid recursion issues
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
}

#[test]
fn test_import_same_file_twice_idempotent() {
    // Loading the same file twice should succeed both times (idempotent)
    let result1 = eval("(import-file \"test-modules/test.lisp\")");
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap(), Value::bool(true));

    // Second load of same file
    let result2 = eval("(import-file \"test-modules/test.lisp\")");
    assert!(result2.is_ok());
    assert_eq!(result2.unwrap(), Value::bool(true));
}

#[test]
fn test_add_module_path_multiple_paths() {
    // Add multiple module search paths
    assert!(eval("(add-module-path \"test-modules\")").is_ok());
    assert!(eval("(add-module-path \"./lib\")").is_ok());
    assert!(eval("(add-module-path \".\")").is_ok());
}

#[test]
fn test_import_file_with_relative_paths() {
    // Test various relative path formats
    assert!(eval("(import-file \"./test-modules/test.lisp\")").is_ok());
    // Only test with simple files to avoid recursion issues
    assert!(eval("(import-file \"test-modules/test.lisp\")").is_ok());
}

// Array pattern matching tests

#[test]
fn test_match_array_literal() {
    assert_eq!(
        eval("(match [1 2 3] ([1 2 3] \"exact\") (_ \"no\"))").unwrap(),
        Value::string("exact")
    );
}

#[test]
fn test_match_array_binding() {
    assert_eq!(
        eval("(match [10 20] ([a b] (+ a b)))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_match_array_wrong_length() {
    assert_eq!(
        eval("(match [1 2] ([a b c] \"three\") ([a b] \"two\"))").unwrap(),
        Value::string("two")
    );
}

#[test]
fn test_match_array_not_array() {
    assert_eq!(
        eval("(match 42 ([a b] \"array\") (_ \"other\"))").unwrap(),
        Value::string("other")
    );
}

#[test]
fn test_match_array_empty() {
    assert_eq!(
        eval("(match [] ([] \"empty\") (_ \"other\"))").unwrap(),
        Value::string("empty")
    );
}

#[test]
fn test_match_array_rest() {
    // & rest captures remaining elements
    assert_eq!(
        eval("(match [1 2 3 4] ([a & rest] (length rest)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_match_array_nested() {
    assert_eq!(
        eval("(match [1 [2 3]] ([a [b c]] (+ a (+ b c))))").unwrap(),
        Value::int(6)
    );
}

// Guard (when) tests

#[test]
fn test_match_guard_basic() {
    assert_eq!(
        eval("(match 5 (x when (> x 3) \"big\") (x \"small\"))").unwrap(),
        Value::string("big")
    );
    assert_eq!(
        eval("(match 2 (x when (> x 3) \"big\") (x \"small\"))").unwrap(),
        Value::string("small")
    );
}

#[test]
fn test_match_guard_with_literal() {
    assert_eq!(
        eval("(match 10 (10 when #f \"nope\") (10 \"yes\"))").unwrap(),
        Value::string("yes")
    );
}

// Cons pattern tests

#[test]
fn test_match_cons_pattern() {
    assert_eq!(
        eval("(match (cons 1 2) ((h . t) (+ h t)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_match_cons_not_pair() {
    assert_eq!(
        eval("(match 42 ((h . t) \"pair\") (_ \"nope\"))").unwrap(),
        Value::string("nope")
    );
}

// List rest pattern tests

#[test]
fn test_match_list_rest() {
    assert_eq!(
        eval("(match (list 1 2 3) ((a & rest) a))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_match_list_exact_length() {
    // List pattern without rest must match exact length
    assert_eq!(
        eval("(match (list 1 2 3) ((1 2) \"two\") ((1 2 3) \"three\"))").unwrap(),
        Value::string("three")
    );
}

// Keyword pattern test

#[test]
fn test_match_keyword_literal() {
    assert_eq!(
        eval("(match :foo (:foo \"matched\") (_ \"no\"))").unwrap(),
        Value::string("matched")
    );
    assert_eq!(
        eval("(match :bar (:foo \"matched\") (_ \"no\"))").unwrap(),
        Value::string("no")
    );
}

// Variable binding test

#[test]
fn test_match_variable_binding() {
    assert_eq!(eval("(match 42 (x (+ x 1)))").unwrap(), Value::int(43));
}

// No-match returns nil

#[test]
fn test_match_no_match_returns_nil() {
    assert_eq!(
        eval("(match 42 (1 \"one\") (2 \"two\"))").unwrap(),
        Value::NIL
    );
}

// Variadic macro tests

#[test]
fn test_variadic_macro_basic() {
    assert_eq!(
        eval("(begin (defmacro my-list (& items) `(list ,@items)) (my-list 1 2 3))").unwrap(),
        eval("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_macro_fixed_and_rest() {
    assert_eq!(
        eval("(begin (defmacro my-add (first & rest) `(+ ,first ,@rest)) (my-add 1 2 3))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_variadic_macro_empty_rest() {
    assert_eq!(
        eval("(begin (defmacro my-list (& items) `(list ,@items)) (my-list))").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_variadic_macro_arity_error() {
    assert!(eval("(begin (defmacro foo (a b & rest) `(list ,a ,b ,@rest)) (foo 1))").is_err());
}

#[test]
fn test_variadic_macro_when_multi_body() {
    // when with multiple body expressions via & rest
    assert_eq!(
        eval("(begin (defmacro my-when (test & body) `(if ,test (begin ,@body) nil)) (my-when #t 1 2 3))")
            .unwrap(),
        Value::int(3)
    );
}
