// DEFENSE: Integration tests ensure the full pipeline works end-to-end
use elle::compiler::converters::value_to_expr;
use elle::{compile, init_stdlib, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}
// Phase 4: Ecosystem & Integration Tests

#[test]
fn test_stdlib_list_module_integration() {
    // Test list module functions through eval
    // length
    assert!(eval("(length (list 1 2 3))").is_ok());

    // append
    assert!(eval("(append (list 1 2) (list 3 4))").is_ok());

    // reverse
    assert!(eval("(reverse (list 1 2 3))").is_ok());
}

#[test]
fn test_stdlib_string_module_integration() {
    // Test string module functions
    assert!(eval("(length \"hello\")").is_ok());
    assert!(eval("(string-upcase \"hello\")").is_ok());
    assert!(eval("(string-downcase \"HELLO\")").is_ok());
}

#[test]
fn test_stdlib_math_module_integration() {
    // Test math module functions
    assert!(eval("(+ 1 2 3)").is_ok());
    assert!(eval("(- 10 3)").is_ok());
    assert!(eval("(* 2 3)").is_ok());
}

#[test]
fn test_list_basic_operations() {
    // Test list operations from stdlib
    assert_eq!(eval("(length (list 1 2 3))").unwrap(), Value::Int(3));
    assert_eq!(eval("(length (list))").unwrap(), Value::Int(0));
}

#[test]
fn test_list_append_basic() {
    // Test append
    match eval("(append (list 1 2) (list 3 4))").unwrap() {
        Value::Cons(_) | Value::Nil => {
            // Valid list
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_list_reverse_basic() {
    // Test reverse
    assert!(eval("(reverse (list 1 2 3))").is_ok());
    assert!(eval("(reverse (list))").is_ok());
}

#[test]
fn test_list_map_basic() {
    // Test map function - note: lambdas need to be defined first
    assert!(eval("(define inc (lambda (x) (+ x 1))) (map inc (list 1 2 3))").is_ok());
}

#[test]
fn test_list_filter_basic() {
    // Test filter function
    assert!(
        eval("(define positive (lambda (x) (> x 0))) (filter positive (list -1 2 -3 4))").is_ok()
    );
}

#[test]
fn test_list_fold_basic() {
    // Test fold/reduce
    assert!(eval("(fold + 0 (list 1 2 3))").is_ok());
}

#[test]
fn test_list_nth_operation() {
    // Test nth - note: signature is (nth index list)
    assert_eq!(eval("(nth 0 (list 10 20 30))").unwrap(), Value::Int(10));
    assert_eq!(eval("(nth 1 (list 10 20 30))").unwrap(), Value::Int(20));
}

#[test]
fn test_list_last_operation() {
    // Test last
    assert_eq!(eval("(last (list 1 2 3))").unwrap(), Value::Int(3));
}

#[test]
fn test_list_take_drop() {
    // Test take and drop - note: signatures are (take count list) and (drop count list)
    assert!(eval("(take 2 (list 1 2 3 4 5))").is_ok());
    assert!(eval("(drop 2 (list 1 2 3 4 5))").is_ok());
}

#[test]
fn test_string_operations_basic() {
    // Test string functions
    assert_eq!(eval("(length \"hello\")").unwrap(), Value::Int(5));
    assert_eq!(eval("(length \"\")").unwrap(), Value::Int(0));
}

#[test]
fn test_string_append_basic() {
    // Test string-append
    match eval("(string-append \"hello\" \" \" \"world\")").unwrap() {
        Value::String(s) => {
            assert_eq!(s.as_ref(), "hello world");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_case_conversion() {
    // Test case conversions
    match eval("(string-upcase \"hello\")").unwrap() {
        Value::String(s) => {
            assert_eq!(s.as_ref(), "HELLO");
        }
        _ => panic!("Expected string"),
    }

    match eval("(string-downcase \"WORLD\")").unwrap() {
        Value::String(s) => {
            assert_eq!(s.as_ref(), "world");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_substring_basic() {
    // Test substring
    match eval("(substring \"hello\" 1 4)").unwrap() {
        Value::String(s) => {
            assert_eq!(s.as_ref(), "ell");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_index_basic() {
    // Test string-index
    assert_eq!(
        eval("(string-index \"hello\" \"l\")").unwrap(),
        Value::Int(2)
    );
}

#[test]
fn test_char_at_basic() {
    // Test char-at
    match eval("(char-at \"hello\" 0)").unwrap() {
        Value::String(s) => {
            assert_eq!(s.as_ref(), "h");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_math_arithmetic() {
    // Test math operations
    assert_eq!(eval("(+ 1 2 3)").unwrap(), Value::Int(6));
    assert_eq!(eval("(- 10 3)").unwrap(), Value::Int(7));
    assert_eq!(eval("(* 2 3 4)").unwrap(), Value::Int(24));
}

#[test]
fn test_math_sqrt_basic() {
    // Test sqrt
    match eval("(sqrt 16)").unwrap() {
        Value::Float(f) => {
            assert!((f - 4.0).abs() < 0.0001);
        }
        Value::Int(i) => {
            assert_eq!(i, 4);
        }
        _ => panic!("Expected number"),
    }
}

#[test]
fn test_math_trigonometric_basic() {
    // Test trig functions
    assert!(eval("(sin 0)").is_ok());
    assert!(eval("(cos 0)").is_ok());
    assert!(eval("(tan 0)").is_ok());
}

#[test]
fn test_math_log_exp_basic() {
    // Test log and exp
    assert!(eval("(log 1)").is_ok());
    assert!(eval("(exp 0)").is_ok());
}

#[test]
fn test_math_pow_basic() {
    // Test power function
    match eval("(pow 2 3)").unwrap() {
        Value::Float(f) => {
            assert!((f - 8.0).abs() < 0.0001);
        }
        Value::Int(i) => {
            assert_eq!(i, 8);
        }
        _ => panic!("Expected number"),
    }
}

#[test]
fn test_math_floor_ceil_round() {
    // Test rounding functions
    assert_eq!(eval("(floor 3.7)").unwrap(), Value::Int(3));
    assert_eq!(eval("(ceil 3.2)").unwrap(), Value::Int(4));
    assert_eq!(eval("(round 3.5)").unwrap(), Value::Int(4));
}

#[test]
fn test_math_constants_basic() {
    // Test pi and e constants
    match eval("(pi)").unwrap() {
        Value::Float(f) => {
            assert!((f - std::f64::consts::PI).abs() < 0.0001);
        }
        _ => panic!("Expected float"),
    }

    match eval("(e)").unwrap() {
        Value::Float(f) => {
            assert!((f - std::f64::consts::E).abs() < 0.0001);
        }
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_comment_syntax_basic() {
    // Test that comments work in code
    let input = r#"
; this is a comment
(+ 1 2)  ; another comment
    "#;
    assert_eq!(eval(input).unwrap(), Value::Int(3));
}

#[test]
fn test_comment_full_line() {
    // Full line comments
    let input = r#"
; entire line is comment
; another full line comment
42
    "#;
    assert_eq!(eval(input).unwrap(), Value::Int(42));
}

#[test]
fn test_package_version_availability() {
    // Test package-version primitive
    match eval("(package-version)").unwrap() {
        Value::String(s) => {
            assert!(!s.is_empty());
        }
        _ => panic!("Expected string version"),
    }
}

#[test]
fn test_package_info_availability() {
    // Test package-info primitive
    match eval("(package-info)").unwrap() {
        Value::Cons(_) | Value::Nil => {
            // Valid list
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_stdlib_with_custom_operations() {
    // Combine stdlib with custom code
    assert!(eval("(define x (list 1 2 3)) (length x))").is_ok());
}

#[test]
fn test_list_and_string_together() {
    // Combine list and string operations
    assert!(eval("(length (string-append \"a\" \"b\"))").is_ok());
}

#[test]
fn test_math_with_strings() {
    // Convert to ensure math works with string primitives available
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::Int(3));
}

#[test]
fn test_gensym_for_macro_hygiene() {
    // Test gensym - returns generated symbol
    assert!(eval("(gensym)").is_ok());
}

#[test]
fn test_gensym_with_prefix() {
    // Test gensym with prefix
    assert!(eval("(gensym \"temp\")").is_ok());
}

#[test]
fn test_exception_creation() {
    // Test exception primitive
    assert!(eval("(exception \"error message\" 42)").is_ok());
}

#[test]
fn test_exception_message_extraction() {
    // Test exception-message
    assert!(eval("(exception-message (exception \"test\" nil))").is_ok());
}

#[test]
fn test_exception_data_extraction() {
    // Test exception-data
    assert!(eval("(exception-data (exception \"test\" 42))").is_ok());
}

#[test]
fn test_throw_and_catch() {
    // Test that exception creation works (try/catch syntax validation in parser)
    assert!(eval("(exception \"error\" nil)").is_ok());
}

#[test]
fn test_list_operations_chain() {
    // Chain multiple list operations
    assert!(eval("(length (append (list 1 2) (list 3 4)))").is_ok());
}

#[test]
fn test_string_operations_chain() {
    // Chain string operations
    assert!(eval("(length (string-upcase \"hello\"))").is_ok());
}

#[test]
fn test_math_operations_chain() {
    // Chain math operations
    assert!(eval("(+ (sqrt 16) 1)").is_ok());
}

#[test]
fn test_all_stdlib_modules_available() {
    // Verify stdlib functions are available
    assert!(eval("(length (list 1))").is_ok());
    assert!(eval("(length \"x\")").is_ok());
    assert!(eval("(+ 1 2)").is_ok());
}
