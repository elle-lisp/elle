// DEFENSE: Integration tests ensure the full pipeline works end-to-end
use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{list, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set symbol table context for primitives that need it (like type-of)
    set_symbol_table(&mut symbols as *mut SymbolTable);

    // Try to compile as a single expression first
    match compile(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            // If that fails, try wrapping in a begin
            let wrapped = format!("(begin {})", input);
            match compile(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    // If that also fails, try compiling all expressions
                    let results = compile_all(input, &mut symbols)?;
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

// Basic arithmetic
#[test]
fn test_simple_arithmetic() {
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::int(3));
    assert_eq!(eval("(- 10 3)").unwrap(), Value::int(7));
    assert_eq!(eval("(* 4 5)").unwrap(), Value::int(20));
    assert_eq!(eval("(/ 20 4)").unwrap(), Value::int(5));
}

#[test]
fn test_nested_arithmetic() {
    assert_eq!(eval("(+ (* 2 3) (- 10 5))").unwrap(), Value::int(11));
    assert_eq!(eval("(* (+ 1 2) (- 5 2))").unwrap(), Value::int(9));
}

#[test]
fn test_deeply_nested() {
    assert_eq!(eval("(+ 1 (+ 2 (+ 3 (+ 4 5))))").unwrap(), Value::int(15));
}

// Comparisons
#[test]
fn test_comparisons() {
    assert_eq!(eval("(= 5 5)").unwrap(), Value::bool(true));
    assert_eq!(eval("(= 5 6)").unwrap(), Value::bool(false));
    assert_eq!(eval("(< 3 5)").unwrap(), Value::bool(true));
    assert_eq!(eval("(< 5 3)").unwrap(), Value::bool(false));
    assert_eq!(eval("(> 7 5)").unwrap(), Value::bool(true));
}

// Conditionals
#[test]
fn test_if_true() {
    assert_eq!(eval("(if #t 100 200)").unwrap(), Value::int(100));
}

#[test]
fn test_if_false() {
    assert_eq!(eval("(if #f 100 200)").unwrap(), Value::int(200));
}

#[test]
fn test_if_with_condition() {
    assert_eq!(eval("(if (> 5 3) 100 200)").unwrap(), Value::int(100));
    assert_eq!(eval("(if (< 5 3) 100 200)").unwrap(), Value::int(200));
}

#[test]
fn test_nested_if() {
    assert_eq!(
        eval("(if (> 5 3) (if (< 2 4) 1 2) 3)").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_if_nil_else() {
    // If without else should return nil
    assert_eq!(eval("(if #f 100)").unwrap(), Value::NIL);
}

// Lists
#[test]
fn test_list_construction() {
    let result = eval("(list 1 2 3)").unwrap();
    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_cons() {
    let result = eval("(cons 1 (cons 2 (cons 3 nil)))").unwrap();
    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(1), Value::int(2), Value::int(3)]);
}

#[test]
fn test_first_rest() {
    assert_eq!(eval("(first (list 10 20 30))").unwrap(), Value::int(10));

    let result = eval("(rest (list 10 20 30))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(20), Value::int(30)]);
}

#[test]
fn test_nested_lists() {
    let result = eval("(list (list 1 2) (list 3 4))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert!(vec[0].is_list());
    assert!(vec[1].is_list());
}

// Quote
#[test]
fn test_quote_symbol() {
    let result = eval("'foo").unwrap();
    assert!((result).is_symbol());
}

#[test]
fn test_quote_list() {
    let result = eval("'(1 2 3)").unwrap();
    assert!(result.is_list());
}

// Type predicates
#[test]
fn test_predicates() {
    assert_eq!(eval("(nil? nil)").unwrap(), Value::bool(true));
    assert_eq!(eval("(nil? 0)").unwrap(), Value::bool(false));

    assert_eq!(eval("(number? 42)").unwrap(), Value::bool(true));
    assert_eq!(eval("(number? nil)").unwrap(), Value::bool(false));

    assert_eq!(eval("(pair? (cons 1 2))").unwrap(), Value::bool(true));
    assert_eq!(eval("(pair? nil)").unwrap(), Value::bool(false));
}

// Global definitions
#[test]
fn test_define_and_use() {
    let code = r#"
        (var x 42)
        (+ x 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(52));
}

#[test]
fn test_multiple_defines() {
    let code = r#"
        (var a 10)
        (var b 20)
        (var c 30)
        (+ a b c)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(60));
}

// Begin
#[test]
fn test_begin() {
    let result = eval("(begin 1 2 3)").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn test_begin_with_side_effects() {
    let code = r#"
        (begin (var x 10) (var y 20) (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

// Complex expressions
#[test]
fn test_factorial_logic() {
    // Simulate factorial without recursion: (if (<= n 1) 1 (* n ...))
    assert_eq!(eval("(if (<= 1 1) 1 (* 1 1))").unwrap(), Value::int(1));

    assert_eq!(eval("(if (<= 5 1) 1 (* 5 120))").unwrap(), Value::int(600));
}

#[test]
fn test_max_logic() {
    assert_eq!(eval("(if (> 10 5) 10 5)").unwrap(), Value::int(10));

    assert_eq!(eval("(if (> 3 7) 3 7)").unwrap(), Value::int(7));
}

// Error cases
#[test]
fn test_division_by_zero() {
    assert!(eval("(/ 10 0)").is_err());
}

#[test]
fn test_type_error() {
    assert!(eval("(+ 1 nil)").is_err());
}

#[test]
fn test_undefined_variable() {
    assert!(eval("undefined-var").is_err());
}

#[test]
fn test_undefined_variable_error_shows_name() {
    // Issue #300: error message should show the variable name, not a SymbolId
    let result = eval("nonexistent-foo");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("nonexistent-foo"),
        "Error should contain variable name, got: {}",
        err
    );
    assert!(
        !err.contains("symbol #"),
        "Error should not contain raw SymbolId, got: {}",
        err
    );
}

#[test]
fn test_arity_error() {
    assert!(eval("(+)").is_ok()); // + accepts 0 args
    assert!(eval("(first)").is_err()); // first requires 1 arg
}

// Stress tests
#[test]
fn test_large_list() {
    // Create list with 100 elements
    let numbers = (0..100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let code = format!("(list {})", numbers);

    let result = eval(&code).unwrap();
    assert!(result.is_list());
    assert_eq!(result.list_to_vec().unwrap().len(), 100);
}

#[test]
fn test_deep_arithmetic() {
    // Test with 50 nested additions
    let mut expr = "1".to_string();
    for _ in 0..50 {
        expr = format!("(+ {} 1)", expr);
    }

    assert_eq!(eval(&expr).unwrap(), Value::int(51));
}

#[test]
fn test_many_operations() {
    // Chain many operations
    assert_eq!(eval("(+ 1 2 3 4 5 6 7 8 9 10)").unwrap(), Value::int(55));

    assert_eq!(eval("(* 1 2 3 4 5)").unwrap(), Value::int(120));
}

// Mixed types
#[test]
fn test_int_float_mixing() {
    if let Some(f) = eval("(+ 1 2.5)").unwrap().as_float() {
        assert!((f - 3.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }

    if let Some(f) = eval("(* 2 3.5)").unwrap().as_float() {
        assert!((f - 7.0).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

// Logic combinations
#[test]
fn test_not() {
    assert_eq!(eval("(not #t)").unwrap(), Value::bool(false));
    assert_eq!(eval("(not #f)").unwrap(), Value::bool(true));
    assert_eq!(eval("(not nil)").unwrap(), Value::bool(true)); // nil is falsy (represents absence)
    assert_eq!(eval("(not ())").unwrap(), Value::bool(false)); // empty list is truthy
    assert_eq!(eval("(not 0)").unwrap(), Value::bool(false)); // 0 is truthy
}

#[test]
fn test_complex_conditionals() {
    assert_eq!(eval("(if (not (< 5 3)) 100 200)").unwrap(), Value::int(100));

    assert!(eval("(if (= (+ 2 3) 5) 'yes 'no)")
        .unwrap()
        .as_symbol()
        .is_some());
}

// New standard library functions
#[test]
fn test_length() {
    assert_eq!(eval("(length (list 1 2 3 4 5))").unwrap(), Value::int(5));
    assert_eq!(eval("(length nil)").unwrap(), Value::int(0));
}

#[test]
fn test_append() {
    let result = eval("(append (list 1 2) (list 3 4) (list 5))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(
        vec,
        vec![
            Value::int(1),
            Value::int(2),
            Value::int(3),
            Value::int(4),
            Value::int(5)
        ]
    );
}

#[test]
fn test_reverse() {
    let result = eval("(reverse (list 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(3), Value::int(2), Value::int(1)]);
}

#[test]
fn test_min_max() {
    assert_eq!(eval("(min 5 3 7 2)").unwrap(), Value::int(2));
    assert_eq!(eval("(max 5 3 7 2)").unwrap(), Value::int(7));

    if let Some(f) = eval("(min 1.5 2 0.5)").unwrap().as_float() {
        assert!((f - 0.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_abs() {
    assert_eq!(eval("(abs -5)").unwrap(), Value::int(5));
    assert_eq!(eval("(abs 5)").unwrap(), Value::int(5));

    if let Some(f) = eval("(abs -3.5)").unwrap().as_float() {
        assert!((f - 3.5).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_type_conversions() {
    assert_eq!(eval("(int 3.14)").unwrap(), Value::int(3));

    if let Some(f) = eval("(float 5)").unwrap().as_float() {
        assert!((f - 5.0).abs() < 1e-10)
    } else {
        panic!("Expected float")
    }
}

// String operations
#[test]
fn test_string_length() {
    assert_eq!(eval("(length \"hello\")").unwrap(), Value::int(5));
    assert_eq!(eval("(length \"\")").unwrap(), Value::int(0));
}

#[test]
fn test_string_append() {
    if let Some(s) = eval("(string-append \"hello\" \" \" \"world\")")
        .unwrap()
        .as_string()
    {
        assert_eq!(s, "hello world")
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_string_case() {
    if let Some(s) = eval("(string-upcase \"hello\")").unwrap().as_string() {
        assert_eq!(s, "HELLO")
    } else {
        panic!("Expected string");
    }

    if let Some(s) = eval("(string-downcase \"WORLD\")").unwrap().as_string() {
        assert_eq!(s, "world")
    } else {
        panic!("Expected string");
    }
}

// List utilities
#[test]
fn test_nth() {
    assert_eq!(eval("(nth 0 (list 10 20 30))").unwrap(), Value::int(10));
    assert_eq!(eval("(nth 1 (list 10 20 30))").unwrap(), Value::int(20));
    assert_eq!(eval("(nth 2 (list 10 20 30))").unwrap(), Value::int(30));
}

#[test]
fn test_last() {
    assert_eq!(eval("(last (list 1 2 3 4 5))").unwrap(), Value::int(5));
}

#[test]
fn test_take_drop() {
    let take_result = eval("(take 2 (list 1 2 3 4 5))").unwrap();
    let take_vec = take_result.list_to_vec().unwrap();
    assert_eq!(take_vec, vec![Value::int(1), Value::int(2)]);

    let drop_result = eval("(drop 2 (list 1 2 3 4 5))").unwrap();
    let drop_vec = drop_result.list_to_vec().unwrap();
    assert_eq!(drop_vec, vec![Value::int(3), Value::int(4), Value::int(5)]);
}

#[test]
fn test_type() {
    let result = eval("(type-of 42)").unwrap();
    if !result.is_keyword() {
        panic!("Expected keyword, got: {:?}", result);
    }

    let result = eval("(type-of 3.14)").unwrap();
    if !result.is_keyword() {
        panic!("Expected keyword, got: {:?}", result);
    }

    let result = eval("(type-of \"hello\")").unwrap();
    if !result.is_keyword() {
        panic!("Expected keyword, got: {:?}", result);
    }
}

#[test]
fn test_type_of_list_consistency() {
    // Issue #308: type-of should return :list for all list-like values
    let empty = eval("(type-of ())").unwrap();
    let proper = eval("(type-of (list 1 2))").unwrap();
    let cons = eval("(type-of (cons 1 2))").unwrap();

    assert!(empty.is_keyword(), "expected keyword for empty list");
    assert!(proper.is_keyword(), "expected keyword for proper list");
    assert!(cons.is_keyword(), "expected keyword for cons cell");

    // All three must return the same keyword
    assert_eq!(
        empty, proper,
        "empty list and proper list should have same type"
    );
    assert_eq!(
        proper, cons,
        "proper list and cons cell should have same type"
    );

    // And that keyword should be :list
    assert_eq!(eval("(eq? (type-of ()) :list)").unwrap(), Value::TRUE);
}

// Math functions
#[test]
fn test_sqrt() {
    assert_eq!(eval("(sqrt 4)").unwrap(), Value::float(2.0));
    assert_eq!(eval("(sqrt 9)").unwrap(), Value::float(3.0));
    // Test with float input
    if let Some(f) = eval("(sqrt 16.0)").unwrap().as_float() {
        assert!((f - 4.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_trigonometric() {
    // sin(0) = 0
    if let Some(f) = eval("(sin 0)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // cos(0) = 1
    if let Some(f) = eval("(cos 0)").unwrap().as_float() {
        assert!((f - 1.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // tan(0) = 0
    if let Some(f) = eval("(tan 0)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_log_functions() {
    // ln(1) = 0
    if let Some(f) = eval("(log 1)").unwrap().as_float() {
        assert!(f.abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // log base 2 of 8 = 3
    if let Some(f) = eval("(log 8 2)").unwrap().as_float() {
        assert!((f - 3.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_exp() {
    // exp(0) = 1
    if let Some(f) = eval("(exp 0)").unwrap().as_float() {
        assert!((f - 1.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // exp(1) ≈ e
    if let Some(f) = eval("(exp 1)").unwrap().as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_pow() {
    // 2^3 = 8
    assert_eq!(eval("(pow 2 3)").unwrap(), Value::int(8));

    // 2^-1 = 0.5
    if let Some(f) = eval("(pow 2 -1)").unwrap().as_float() {
        assert!((f - 0.5).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // 2.0^3.0 = 8.0
    if let Some(f) = eval("(pow 2.0 3.0)").unwrap().as_float() {
        assert!((f - 8.0).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_floor_ceil_round() {
    // floor
    assert_eq!(eval("(floor 3)").unwrap(), Value::int(3));
    assert_eq!(eval("(floor 3.7)").unwrap(), Value::int(3));

    // ceil
    assert_eq!(eval("(ceil 3)").unwrap(), Value::int(3));
    assert_eq!(eval("(ceil 3.2)").unwrap(), Value::int(4));

    // round
    assert_eq!(eval("(round 3)").unwrap(), Value::int(3));
    assert_eq!(eval("(round 3.4)").unwrap(), Value::int(3));
    assert_eq!(eval("(round 3.6)").unwrap(), Value::int(4));
}

// String functions
#[test]
fn test_substring() {
    if let Some(s) = eval("(substring \"hello\" 1 4)").unwrap().as_string() {
        assert_eq!(s, "ell")
    } else {
        panic!("Expected string");
    }

    // Test with just start index (to end)
    if let Some(s) = eval("(substring \"hello\" 2)").unwrap().as_string() {
        assert_eq!(s, "llo")
    } else {
        panic!("Expected string");
    }

    // Test from start
    if let Some(s) = eval("(substring \"hello\" 0 2)").unwrap().as_string() {
        assert_eq!(s, "he")
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_string_index() {
    // Find character in string
    assert_eq!(
        eval("(string-index \"hello\" \"l\")").unwrap(),
        Value::int(2)
    );

    // Character not found
    assert_eq!(eval("(string-index \"hello\" \"x\")").unwrap(), Value::NIL);

    // First occurrence
    assert_eq!(
        eval("(string-index \"hello\" \"l\")").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_char_at() {
    if let Some(s) = eval("(char-at \"hello\" 0)").unwrap().as_string() {
        assert_eq!(s, "h")
    } else {
        panic!("Expected string");
    }

    if let Some(s) = eval("(char-at \"hello\" 1)").unwrap().as_string() {
        assert_eq!(s, "e")
    } else {
        panic!("Expected string");
    }

    if let Some(s) = eval("(char-at \"hello\" 4)").unwrap().as_string() {
        assert_eq!(s, "o")
    } else {
        panic!("Expected string");
    }
}

// Array operations
#[test]
fn test_array_creation() {
    if let Some(vec_ref) = eval("(array 1 2 3)").unwrap().as_array() {
        let v = vec_ref.borrow();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Value::int(1));
        assert_eq!(v[1], Value::int(2));
        assert_eq!(v[2], Value::int(3));
    } else {
        panic!("Expected array");
    }

    // Empty array
    if let Some(vec_ref) = eval("(array)").unwrap().as_array() {
        assert_eq!(vec_ref.borrow().len(), 0)
    } else {
        panic!("Expected array");
    }
}

#[test]
fn test_array_length() {
    assert_eq!(eval("(length (array 1 2 3))").unwrap(), Value::int(3));
    assert_eq!(eval("(length (array))").unwrap(), Value::int(0));
    assert_eq!(
        eval("(length (array 10 20 30 40 50))").unwrap(),
        Value::int(5)
    );
}

#[test]
fn test_array_ref() {
    assert_eq!(
        eval("(array-ref (array 10 20 30) 0)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval("(array-ref (array 10 20 30) 1)").unwrap(),
        Value::int(20)
    );
    assert_eq!(
        eval("(array-ref (array 10 20 30) 2)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_array_set() {
    if let Some(vec_ref) = eval("(array-set! (array 1 2 3) 1 99)").unwrap().as_array() {
        let v = vec_ref.borrow();
        assert_eq!(v[0], Value::int(1));
        assert_eq!(v[1], Value::int(99));
        assert_eq!(v[2], Value::int(3));
    } else {
        panic!("Expected array");
    }

    // Set at beginning
    if let Some(vec_ref) = eval("(array-set! (array 1 2 3) 0 100)").unwrap().as_array() {
        let v = vec_ref.borrow();
        assert_eq!(v[0], Value::int(100))
    } else {
        panic!("Expected array")
    }

    // Set at end
    if let Some(vec_ref) = eval("(array-set! (array 1 2 3) 2 200)").unwrap().as_array() {
        let v = vec_ref.borrow();
        assert_eq!(v[2], Value::int(200))
    } else {
        panic!("Expected array")
    }
}
#[test]
fn test_math_constants() {
    // Test pi
    if let Some(f) = eval("(pi)").unwrap().as_float() {
        assert!((f - std::f64::consts::PI).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }

    // Test e
    if let Some(f) = eval("(e)").unwrap().as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.0001)
    } else {
        panic!("Expected float")
    }
}

#[test]
fn test_mod_and_remainder() {
    // Modulo
    assert_eq!(eval("(mod 17 5)").unwrap(), Value::int(2));
    assert_eq!(eval("(mod 20 4)").unwrap(), Value::int(0));
    assert_eq!(eval("(mod -17 5)").unwrap(), Value::int(3));

    // Remainder
    assert_eq!(eval("(rem 17 5)").unwrap(), Value::int(2));
    assert_eq!(eval("(rem 20 4)").unwrap(), Value::int(0));
}

#[test]
fn test_even_odd() {
    assert_eq!(eval("(even? 2)").unwrap(), Value::bool(true));
    assert_eq!(eval("(even? 3)").unwrap(), Value::bool(false));
    assert_eq!(eval("(odd? 2)").unwrap(), Value::bool(false));
    assert_eq!(eval("(odd? 3)").unwrap(), Value::bool(true));
    assert_eq!(eval("(even? 0)").unwrap(), Value::bool(true));
}

// Recursive function tests (issue #6)
// Note: These tests demonstrate the expected behavior for recursive lambdas
// Full support requires forward reference mechanism or circular reference handling
// See PR #13 for partial implementation

#[test]
fn test_recursive_lambda_fibonacci() {
    // Test basic recursive lambda
    let code = r#"
        (defn fib (n) 
          (if (< n 2) 
            n 
            (+ (fib (- n 1)) (fib (- n 2)))))
        (fib 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_recursive_lambda_fibonacci_10() {
    // Test fibonacci(10) = 55
    let code = r#"
        (defn fib (n) 
          (if (< n 2) 
            n 
            (+ (fib (- n 1)) (fib (- n 2)))))
        (fib 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(55));
}

#[test]
fn test_tail_recursive_sum() {
    // Test tail-recursive sum accumulation
    let code = r#"
        (defn sum-to (n acc)
          (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
        (sum-to 100 0)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5050));
}

#[test]
fn test_recursive_countdown() {
    // Test simple countdown recursion
    let code = r#"
        (defn countdown (n)
          (if (<= n 0)
            0
            (+ n (countdown (- n 1)))))
        (countdown 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15)); // 5 + 4 + 3 + 2 + 1
}

#[test]
fn test_nested_recursive_functions() {
    // Test nested function definitions with recursion
    let code = r#"
        (defn outer (n)
          (defn inner (x)
            (if (< x 1) 0 (+ x (inner (- x 1)))))
          (inner n))
        (outer 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15)); // 5 + 4 + 3 + 2 + 1
}

#[test]
fn test_simple_lambda_call() {
    // Simple lambda that takes a parameter and returns it
    let code = r#"
        (defn identity (x) x)
        (identity 42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_lambda_with_arithmetic() {
    // Lambda that does arithmetic on its parameter
    let code = r#"
        (defn double (x) (* x 2))
        (double 21)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_lambda_with_comparison() {
    // Lambda that uses comparison on its parameter
    let code = r#"
        (defn is-positive (x) (> x 0))
        (is-positive 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

// Closure scoping tests (Issue #21 - Foundation work on closure improvements)
// These tests demonstrate working closure parameter scoping with lambda expressions

#[test]
fn test_closure_captures_outer_variable() {
    let code = r#"
        (var x 100)
        ((fn (y) (+ x y)) 20)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(120));
}

#[test]
fn test_closure_parameter_shadowing() {
    let code = r#"
        (var x 100)
        ((fn (x) (+ x 1)) 50)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(51));
}

#[test]
fn test_closure_captures_multiple_variables() {
    let code = r#"
        (var x 10)
        (var y 20)
        (var z 30)
        ((fn (a b c) (+ a b c x y z)) 1 2 3)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(66));
}

#[test]
fn test_closure_parameter_in_nested_expression() {
    let code = r#"
        ((fn (x)
          (if (> x 50) (* x 2) (+ x 100))) 25)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(125));
}

#[test]
fn test_multiple_closures_independent_params() {
    let code = r#"
        (def f1 (fn (x) (+ x 10)))
        (def f2 (fn (x) (* x 2)))
        (+ (f1 5) (f2 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(25));
}

#[test]
fn test_closure_captured_function_call() {
    let code = r#"
        (def add (fn (a b) (+ a b)))
        ((fn (x y) (add x y)) 10 20)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_closure_with_list_operations() {
    let code = r#"
        (var numbers (list 1 2 3 4 5))
        ((fn (lst) (first lst)) numbers)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(1));
}

#[test]
fn test_closure_parameter_in_conditional() {
    let code = r#"
        ((fn (n)
          (if (nil? n) "empty" "nonempty"))
         (list 1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::string("nonempty".to_string()));
}

#[test]
fn test_closure_preserves_parameter_type() {
    let code = r#"
        ((fn (s) (string? s)) "hello")
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

// Let-binding tests (Issue #21)

#[test]
fn test_let_simple_binding() {
    let code = r#"
        (let ((x 5))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_let_with_arithmetic() {
    let code = r#"
        (let ((x 5))
          (+ x 3))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(8));
}

#[test]
fn test_let_multiple_bindings() {
    let code = r#"
        (let ((x 5) (y 3))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(8));
}

#[test]
fn test_let_binding_with_expressions() {
    let code = r#"
        (let ((x (+ 2 3)) (y (* 4 5)))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(25));
}

#[test]
fn test_let_shadowing_global() {
    let code = r#"
        (var x 10)
        (let ((x 20))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

#[test]
fn test_let_does_not_modify_global() {
    let code = r#"
        (var x 10)
        (let ((x 20))
          x)
        x
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

#[test]
fn test_let_with_lists() {
    let code = r#"
        (let ((lst (list 1 2 3)))
          (first lst))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(1));
}

#[test]
fn test_let_with_string_operations() {
    let code = r#"
        (let ((s "hello"))
          (string? s))
    "#;
    assert_eq!(eval(code).unwrap(), Value::bool(true));
}

#[test]
fn test_let_with_conditional() {
    let code = r#"
        (let ((x 10))
          (if (> x 5) "big" "small"))
    "#;
    assert_eq!(eval(code).unwrap(), Value::string("big".to_string()));
}

#[test]
fn test_let_empty_body_returns_nil() {
    let code = r#"
        (let ((x 5)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::NIL);
}

#[test]
fn test_let_multiple_body_expressions() {
    let code = r#"
        (let ((x 5))
          (+ x 1)
          (+ x 2)
          (+ x 3))
    "#;
    // Body should return last expression
    assert_eq!(eval(code).unwrap(), Value::int(8));
}

#[test]
fn test_let_with_global_reference() {
    let code = r#"
        (var y 100)
        (let ((x 50))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(150));
}

#[test]
fn test_let_binding_order() {
    let code = r#"
        (let ((x 1) (y 2) (z 3))
          (+ x y z))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_let_with_list_literal() {
    let code = r#"
        (let ((x (quote (1 2 3))))
          (rest x))
    "#;
    assert_eq!(
        eval(code).unwrap(),
        list(vec![Value::int(2), Value::int(3)])
    );
}

#[test]
fn test_let_shadowing_with_calculation() {
    let code = r#"
        (var x 10)
        (let ((x (* 2 x)))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(20));
}

#[test]
fn test_let_with_builtin_functions() {
    let code = r#"
         (let ((len (fn (x) 42)))
           (len nil))
     "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

// Tests for let* (sequential binding with access to previous bindings)

#[test]
fn test_let_star_empty() {
    let code = r#"
        (let* ()
          42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_let_star_simple_binding() {
    let code = r#"
        (let* ((x 5))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_let_star_with_multiple_bindings_no_dependencies() {
    let code = r#"
        (let* ((x 1) (y 2))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

// Tests for cond expression (multi-way conditional)

#[test]
fn test_cond_single_true_clause() {
    assert_eq!(eval("(cond (#t 42))").unwrap(), Value::int(42));
}

#[test]
fn test_cond_single_false_clause_with_else() {
    assert_eq!(eval("(cond (#f 42) (else 100))").unwrap(), Value::int(100));
}

#[test]
fn test_cond_single_false_clause_without_else() {
    // If no clause matches and no else, return nil
    assert_eq!(eval("(cond (#f 42))").unwrap(), Value::NIL);
}

#[test]
fn test_cond_first_clause_matches() {
    assert_eq!(
        eval("(cond ((> 5 3) 100) ((> 4 2) 200))").unwrap(),
        Value::int(100)
    );
}

#[test]
fn test_cond_second_clause_matches() {
    assert_eq!(
        eval("(cond ((> 3 5) 100) ((> 4 2) 200))").unwrap(),
        Value::int(200)
    );
}

#[test]
fn test_cond_multiple_clauses_with_else() {
    assert_eq!(
        eval("(cond ((> 3 5) 100) ((> 2 4) 200) (else 300))").unwrap(),
        Value::int(300)
    );
}

#[test]
fn test_cond_with_expressions_as_conditions() {
    let code = r#"
        (cond
          ((= 1 2) "one-two")
          ((= 2 2) "two-two")
          (else "other"))
    "#;
    if let Some(s) = eval(code).unwrap().as_string() {
        assert_eq!(s, "two-two")
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_cond_with_complex_bodies() {
    let code = r#"
        (cond
          (#f (+ 1 1))
          (#t (+ 2 3))
          (else (+ 4 5)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(5));
}

#[test]
fn test_cond_with_multiple_body_expressions() {
    // Cond body can have multiple expressions, returns the last one
    let code = r#"
        (cond
          (#t
            (+ 1 1)
            (+ 2 2)
            (+ 3 3)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_cond_nested() {
    let code = r#"
        (cond
          (#t
            (cond
              (#t 42)
              (else 100)))
          (else 200))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_cond_with_variable_references() {
    let code = r#"
        (var x 10)
        (cond
          ((< x 5) "small")
          ((< x 15) "medium")
          (else "large"))
    "#;
    if let Some(s) = eval(code).unwrap().as_string() {
        assert_eq!(s, "medium")
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_cond_respects_clause_order() {
    // The first matching clause wins
    let code = r#"
        (cond
          ((>= 10 5) "first")
          ((>= 10 3) "second")
          (else "third"))
    "#;
    if let Some(s) = eval(code).unwrap().as_string() {
        assert_eq!(s, "first")
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_cond_with_else_body_multiple_expressions() {
    let code = r#"
        (cond
          (#f 100)
          (else
            (+ 1 1)
            (+ 2 2)
            (* 3 3)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(9));
}

// Tests for nested lambdas with closure capture
// Note: These are limited scope tests due to known issues #78 with parameter access
// when captures are present. Only tests that access captures without parameter interference work.

#[test]
fn test_nested_lambda_single_capture() {
    // Test that nested lambdas can access outer lambda parameters (capture only)
    let code = r#"
        (def make-const (fn (x)
          (fn (y)
            x)))
        
        (var f (make-const 42))
        (f 100)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_nested_lambda_parameter_only() {
    // Test that nested lambdas can access their own parameters (no captures)
    let code = r#"
         (def make-id (fn (x)
           (fn (y)
             y)))
         
         (var f (make-id 100))
         (f 42)
     "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

// TODO: Fix issue #78 - Deeply nested closures with multiple captures
// The following tests are disabled because they currently fail due to incorrect
// index resolution in closures with 3+ nesting levels.
//
// #[test]
// fn test_triple_nested_closure_with_multiple_captures() {
//     let code = r#"
//          (def make-multiplier (fn (a)
//            (fn (b)
//              (fn (c)
//                (* a (* b c))))))
//          (((make-multiplier 2) 3) 4)
//      "#;
//     assert_eq!(eval(code).unwrap(), Value::int(24));
// }

// Threading operators (-> and ->>)
#[test]
fn test_thread_first_simple() {
    // (-> 5 (+ 10) (* 2)) => (* (+ 5 10) 2) => 30
    let code = "(-> 5 (+ 10) (* 2))";
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_thread_first_with_multiple_args() {
    // (-> 5 (+ 10 2) (* 3)) => (* (+ 5 10 2) 3) => 51
    let code = "(-> 5 (+ 10 2) (* 3))";
    assert_eq!(eval(code).unwrap(), Value::int(51));
}

#[test]
fn test_thread_last_simple() {
    // (->> 5 (+ 10) (* 2)) => (* 2 (+ 10 5)) => 30
    let code = "(->> 5 (+ 10) (* 2))";
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_thread_last_with_multiple_args() {
    // (->> 2 (+ 10) (* 3)) => (* 3 (+ 10 2)) => 36
    let code = "(->> 2 (+ 10) (* 3))";
    assert_eq!(eval(code).unwrap(), Value::int(36));
}

#[test]
fn test_thread_first_chain() {
    // (-> 1 (+ 1) (+ 1) (+ 1)) => (+ (+ (+ 1 1) 1) 1) => 4
    let code = "(-> 1 (+ 1) (+ 1) (+ 1))";
    assert_eq!(eval(code).unwrap(), Value::int(4));
}

#[test]
fn test_thread_last_chain() {
    // (->> 1 (+ 1) (+ 1) (+ 1)) => (+ 1 (+ 1 (+ 1 1))) => 4
    let code = "(->> 1 (+ 1) (+ 1) (+ 1))";
    assert_eq!(eval(code).unwrap(), Value::int(4));
}

#[test]
fn test_thread_first_with_list_ops() {
    // (-> (list 1 2 3) (length)) => (length (list 1 2 3)) => 3
    let code = "(-> (list 1 2 3) (length))";
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_thread_last_with_list_ops() {
    // (->> (list 1 2 3) (length)) => (length (list 1 2 3)) => 3
    let code = "(->> (list 1 2 3) (length))";
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_thread_first_nested() {
    // Test threading through nested operations
    let code = "(-> 10 (- 3) (+ 5))";
    // (+ (- 10 3) 5) = (+ 7 5) = 12
    assert_eq!(eval(code).unwrap(), Value::int(12));
}

#[test]
fn test_thread_last_nested() {
    // Test threading through nested operations
    let code = "(->> 10 (- 3) (+ 5))";
    // (+ 5 (- 3 10)) = (+ 5 -7) = -2
    assert_eq!(eval(code).unwrap(), Value::int(-2));
}

#[test]
fn test_closure_with_local_define_and_param_arithmetic() {
    // Issue: inner closure accessing both captured local and its own parameter
    // The inner closure captures `local` and has parameter `y`
    // Environment layout should be: [local, y] at indices [0, 1]
    let code = r#"
        (let ((outer (fn (x) 
                       (begin 
                         (var local (* x 2)) 
                         (fn (y) (+ local y))))))
          ((outer 1) 1))
    "#;
    // local = 1 * 2 = 2, y = 1, result = 2 + 1 = 3
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

// ============================================================================
// Bug fix tests: StoreCapture stack mismatch
// ============================================================================

#[test]
fn test_let_inside_lambda_with_append() {
    let result =
        eval("(defn f (x) (if (= x 0) (list) (let ((y x)) (append (list y) (f (- x 1)))))) (f 3)");
    // Should be (3 2 1)
    assert!(result.is_ok());
    let val = result.unwrap();
    let vec = val.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(3), Value::int(2), Value::int(1)]);
}

#[test]
fn test_let_inside_lambda_values_correct() {
    let result = eval("(defn f (x) (let ((y x)) y)) (f 42)");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_multiple_let_bindings_in_lambda() {
    let result = eval("(defn f (x) (let ((y x) (z (+ x 1))) (+ y z))) (f 10)");
    assert_eq!(result.unwrap(), Value::int(21));
}

// ============================================================================
// Bug fix tests: defn
// ============================================================================

#[test]
fn test_define_shorthand() {
    let result = eval("(defn f (x) (+ x 1)) (f 42)");
    assert_eq!(result.unwrap(), Value::int(43));
}

#[test]
fn test_define_shorthand_multiple_params() {
    let result = eval("(defn add (a b) (+ a b)) (add 3 4)");
    assert_eq!(result.unwrap(), Value::int(7));
}

#[test]
fn test_define_shorthand_with_body() {
    let result = eval("(defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact 5)");
    assert_eq!(result.unwrap(), Value::int(120));
}

// ============================================================================
// Bug fix tests: List printing shows `. ()`
// ============================================================================

#[test]
fn test_list_display_no_dot() {
    let result = eval("(list 1 2 3)");
    let val = result.unwrap();
    assert_eq!(format!("{}", val), "(1 2 3)");
}

#[test]
fn test_single_element_list_display() {
    let result = eval("(list 1)");
    let val = result.unwrap();
    assert_eq!(format!("{}", val), "(1)");
}

#[test]
fn test_empty_list_display() {
    let result = eval("(list)");
    let val = result.unwrap();
    assert_eq!(format!("{}", val), "()");
}

// ============================================================================
// halt primitive
// ============================================================================

#[test]
fn test_halt_returns_value() {
    let result = eval("(halt 42)");
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_halt_returns_nil() {
    let result = eval("(halt)");
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_halt_stops_execution() {
    // Code after halt should not execute
    let result = eval("(begin (halt 1) 2)");
    assert_eq!(result.unwrap(), Value::int(1));
}

#[test]
fn test_halt_in_function() {
    let result = eval("(begin (def f (fn () (halt 99))) (f))");
    assert_eq!(result.unwrap(), Value::int(99));
}

#[test]
fn test_halt_with_complex_value() {
    let result = eval("(halt (list 1 2 3))");
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::int(1), Value::int(2), Value::int(3)]);
}
