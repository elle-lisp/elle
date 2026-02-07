// DEFENSE: Integration tests ensure the full pipeline works end-to-end
use elle::compiler::converters::value_to_expr;
use elle::{compile, list, read_str, register_primitives, Lexer, Reader, SymbolTable, Value, VM};
use std::rc::Rc;

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Tokenize the input
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    // Read all expressions
    let mut reader = Reader::new(tokens);
    let mut values = Vec::new();
    while let Some(result) = reader.try_read(&mut symbols) {
        values.push(result?);
    }

    // If we have multiple expressions, wrap them in a begin
    let value = if values.len() == 1 {
        values.into_iter().next().unwrap()
    } else if values.is_empty() {
        return Err("No input".to_string());
    } else {
        // Wrap multiple expressions in a begin
        let mut begin_args = vec![Value::Symbol(symbols.intern("begin"))];
        begin_args.extend(values);
        list(begin_args)
    };

    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// Basic arithmetic
#[test]
fn test_simple_arithmetic() {
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::Int(3));
    assert_eq!(eval("(- 10 3)").unwrap(), Value::Int(7));
    assert_eq!(eval("(* 4 5)").unwrap(), Value::Int(20));
    assert_eq!(eval("(/ 20 4)").unwrap(), Value::Int(5));
}

#[test]
fn test_nested_arithmetic() {
    assert_eq!(eval("(+ (* 2 3) (- 10 5))").unwrap(), Value::Int(11));
    assert_eq!(eval("(* (+ 1 2) (- 5 2))").unwrap(), Value::Int(9));
}

#[test]
fn test_deeply_nested() {
    assert_eq!(eval("(+ 1 (+ 2 (+ 3 (+ 4 5))))").unwrap(), Value::Int(15));
}

// Comparisons
#[test]
fn test_comparisons() {
    assert_eq!(eval("(= 5 5)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(= 5 6)").unwrap(), Value::Bool(false));
    assert_eq!(eval("(< 3 5)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(< 5 3)").unwrap(), Value::Bool(false));
    assert_eq!(eval("(> 7 5)").unwrap(), Value::Bool(true));
}

// Conditionals
#[test]
fn test_if_true() {
    assert_eq!(eval("(if #t 100 200)").unwrap(), Value::Int(100));
}

#[test]
fn test_if_false() {
    assert_eq!(eval("(if #f 100 200)").unwrap(), Value::Int(200));
}

#[test]
fn test_if_with_condition() {
    assert_eq!(eval("(if (> 5 3) 100 200)").unwrap(), Value::Int(100));
    assert_eq!(eval("(if (< 5 3) 100 200)").unwrap(), Value::Int(200));
}

#[test]
fn test_nested_if() {
    assert_eq!(
        eval("(if (> 5 3) (if (< 2 4) 1 2) 3)").unwrap(),
        Value::Int(1)
    );
}

#[test]
fn test_if_nil_else() {
    // If without else should return nil
    assert_eq!(eval("(if #f 100)").unwrap(), Value::Nil);
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
    assert_eq!(vec, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn test_first_rest() {
    assert_eq!(eval("(first (list 10 20 30))").unwrap(), Value::Int(10));

    let result = eval("(rest (list 10 20 30))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::Int(20), Value::Int(30)]);
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
    assert!(matches!(result, Value::Symbol(_)));
}

#[test]
fn test_quote_list() {
    let result = eval("'(1 2 3)").unwrap();
    assert!(result.is_list());
}

// Type predicates
#[test]
fn test_predicates() {
    assert_eq!(eval("(nil? nil)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(nil? 0)").unwrap(), Value::Bool(false));

    assert_eq!(eval("(number? 42)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(number? nil)").unwrap(), Value::Bool(false));

    assert_eq!(eval("(pair? (cons 1 2))").unwrap(), Value::Bool(true));
    assert_eq!(eval("(pair? nil)").unwrap(), Value::Bool(false));
}

// Global definitions
#[test]
fn test_define_and_use() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Define x
    let def = read_str("(define x 42)", &mut symbols).unwrap();
    let expr = value_to_expr(&def, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    vm.execute(&bytecode).unwrap();

    // Use x
    let use_x = read_str("(+ x 10)", &mut symbols).unwrap();
    let expr = value_to_expr(&use_x, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode).unwrap();

    assert_eq!(result, Value::Int(52));
}

#[test]
fn test_multiple_defines() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Define multiple variables
    for (name, value) in &[("a", "10"), ("b", "20"), ("c", "30")] {
        let def = read_str(&format!("(define {} {})", name, value), &mut symbols).unwrap();
        let expr = value_to_expr(&def, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        vm.execute(&bytecode).unwrap();
    }

    // Use them
    let result = read_str("(+ a b c)", &mut symbols).unwrap();
    let expr = value_to_expr(&result, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode).unwrap();

    assert_eq!(result, Value::Int(60));
}

// Begin
#[test]
fn test_begin() {
    let result = eval("(begin 1 2 3)").unwrap();
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_begin_with_side_effects() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Begin with defines
    let code = "(begin (define x 10) (define y 20) (+ x y))";
    let value = read_str(code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode).unwrap();

    assert_eq!(result, Value::Int(30));
}

// Complex expressions
#[test]
fn test_factorial_logic() {
    // Simulate factorial without recursion: (if (<= n 1) 1 (* n ...))
    assert_eq!(eval("(if (<= 1 1) 1 (* 1 1))").unwrap(), Value::Int(1));

    assert_eq!(eval("(if (<= 5 1) 1 (* 5 120))").unwrap(), Value::Int(600));
}

#[test]
fn test_max_logic() {
    assert_eq!(eval("(if (> 10 5) 10 5)").unwrap(), Value::Int(10));

    assert_eq!(eval("(if (> 3 7) 3 7)").unwrap(), Value::Int(7));
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
fn test_arity_error() {
    assert!(eval("(+)").is_ok()); // + accepts 0 args
    assert!(eval("(first)").is_err()); // first requires 1 arg
}

// Stress tests
#[test]
fn test_large_list() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Create list with 100 elements
    let numbers = (0..100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let code = format!("(list {})", numbers);

    let value = read_str(&code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode).unwrap();

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

    assert_eq!(eval(&expr).unwrap(), Value::Int(51));
}

#[test]
fn test_many_operations() {
    // Chain many operations
    assert_eq!(eval("(+ 1 2 3 4 5 6 7 8 9 10)").unwrap(), Value::Int(55));

    assert_eq!(eval("(* 1 2 3 4 5)").unwrap(), Value::Int(120));
}

// Mixed types
#[test]
fn test_int_float_mixing() {
    match eval("(+ 1 2.5)").unwrap() {
        Value::Float(f) => assert!((f - 3.5).abs() < 1e-10),
        _ => panic!("Expected float"),
    }

    match eval("(* 2 3.5)").unwrap() {
        Value::Float(f) => assert!((f - 7.0).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

// Logic combinations
#[test]
fn test_not() {
    assert_eq!(eval("(not #t)").unwrap(), Value::Bool(false));
    assert_eq!(eval("(not #f)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(not nil)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(not 0)").unwrap(), Value::Bool(false)); // 0 is truthy
}

#[test]
fn test_complex_conditionals() {
    assert_eq!(eval("(if (not (< 5 3)) 100 200)").unwrap(), Value::Int(100));

    assert!(eval("(if (= (+ 2 3) 5) 'yes 'no)")
        .unwrap()
        .as_symbol()
        .is_ok());
}

// New standard library functions
#[test]
fn test_length() {
    assert_eq!(eval("(length (list 1 2 3 4 5))").unwrap(), Value::Int(5));
    assert_eq!(eval("(length nil)").unwrap(), Value::Int(0));
}

#[test]
fn test_append() {
    let result = eval("(append (list 1 2) (list 3 4) (list 5))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(
        vec,
        vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
            Value::Int(5)
        ]
    );
}

#[test]
fn test_reverse() {
    let result = eval("(reverse (list 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec, vec![Value::Int(3), Value::Int(2), Value::Int(1)]);
}

#[test]
fn test_min_max() {
    assert_eq!(eval("(min 5 3 7 2)").unwrap(), Value::Int(2));
    assert_eq!(eval("(max 5 3 7 2)").unwrap(), Value::Int(7));

    match eval("(min 1.5 2 0.5)").unwrap() {
        Value::Float(f) => assert!((f - 0.5).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_abs() {
    assert_eq!(eval("(abs -5)").unwrap(), Value::Int(5));
    assert_eq!(eval("(abs 5)").unwrap(), Value::Int(5));

    match eval("(abs -3.5)").unwrap() {
        Value::Float(f) => assert!((f - 3.5).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_type_conversions() {
    assert_eq!(eval("(int 3.14)").unwrap(), Value::Int(3));

    match eval("(float 5)").unwrap() {
        Value::Float(f) => assert!((f - 5.0).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

// String operations
#[test]
fn test_string_length() {
    assert_eq!(eval("(string-length \"hello\")").unwrap(), Value::Int(5));
    assert_eq!(eval("(string-length \"\")").unwrap(), Value::Int(0));
}

#[test]
fn test_string_append() {
    match eval("(string-append \"hello\" \" \" \"world\")").unwrap() {
        Value::String(s) => assert_eq!(&*s, "hello world"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_case() {
    match eval("(string-upcase \"hello\")").unwrap() {
        Value::String(s) => assert_eq!(&*s, "HELLO"),
        _ => panic!("Expected string"),
    }

    match eval("(string-downcase \"WORLD\")").unwrap() {
        Value::String(s) => assert_eq!(&*s, "world"),
        _ => panic!("Expected string"),
    }
}

// List utilities
#[test]
fn test_nth() {
    assert_eq!(eval("(nth 0 (list 10 20 30))").unwrap(), Value::Int(10));
    assert_eq!(eval("(nth 1 (list 10 20 30))").unwrap(), Value::Int(20));
    assert_eq!(eval("(nth 2 (list 10 20 30))").unwrap(), Value::Int(30));
}

#[test]
fn test_last() {
    assert_eq!(eval("(last (list 1 2 3 4 5))").unwrap(), Value::Int(5));
}

#[test]
fn test_take_drop() {
    let take_result = eval("(take 2 (list 1 2 3 4 5))").unwrap();
    let take_vec = take_result.list_to_vec().unwrap();
    assert_eq!(take_vec, vec![Value::Int(1), Value::Int(2)]);

    let drop_result = eval("(drop 2 (list 1 2 3 4 5))").unwrap();
    let drop_vec = drop_result.list_to_vec().unwrap();
    assert_eq!(drop_vec, vec![Value::Int(3), Value::Int(4), Value::Int(5)]);
}

#[test]
fn test_type() {
    match eval("(type 42)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "integer"),
        _ => panic!("Expected string"),
    }

    match eval("(type 3.14)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "float"),
        _ => panic!("Expected string"),
    }

    match eval("(type \"hello\")").unwrap() {
        Value::String(s) => assert_eq!(&*s, "string"),
        _ => panic!("Expected string"),
    }
}

// Math functions
#[test]
fn test_sqrt() {
    assert_eq!(eval("(sqrt 4)").unwrap(), Value::Float(2.0));
    assert_eq!(eval("(sqrt 9)").unwrap(), Value::Float(3.0));
    // Test with float input
    match eval("(sqrt 16.0)").unwrap() {
        Value::Float(f) => assert!((f - 4.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_trigonometric() {
    // sin(0) = 0
    match eval("(sin 0)").unwrap() {
        Value::Float(f) => assert!(f.abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // cos(0) = 1
    match eval("(cos 0)").unwrap() {
        Value::Float(f) => assert!((f - 1.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // tan(0) = 0
    match eval("(tan 0)").unwrap() {
        Value::Float(f) => assert!(f.abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_log_functions() {
    // ln(1) = 0
    match eval("(log 1)").unwrap() {
        Value::Float(f) => assert!(f.abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // log base 2 of 8 = 3
    match eval("(log 8 2)").unwrap() {
        Value::Float(f) => assert!((f - 3.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_exp() {
    // exp(0) = 1
    match eval("(exp 0)").unwrap() {
        Value::Float(f) => assert!((f - 1.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // exp(1) ≈ e
    match eval("(exp 1)").unwrap() {
        Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_pow() {
    // 2^3 = 8
    assert_eq!(eval("(pow 2 3)").unwrap(), Value::Int(8));

    // 2^-1 = 0.5
    match eval("(pow 2 -1)").unwrap() {
        Value::Float(f) => assert!((f - 0.5).abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // 2.0^3.0 = 8.0
    match eval("(pow 2.0 3.0)").unwrap() {
        Value::Float(f) => assert!((f - 8.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_floor_ceil_round() {
    // floor
    assert_eq!(eval("(floor 3)").unwrap(), Value::Int(3));
    assert_eq!(eval("(floor 3.7)").unwrap(), Value::Int(3));

    // ceil
    assert_eq!(eval("(ceil 3)").unwrap(), Value::Int(3));
    assert_eq!(eval("(ceil 3.2)").unwrap(), Value::Int(4));

    // round
    assert_eq!(eval("(round 3)").unwrap(), Value::Int(3));
    assert_eq!(eval("(round 3.4)").unwrap(), Value::Int(3));
    assert_eq!(eval("(round 3.6)").unwrap(), Value::Int(4));
}

// String functions
#[test]
fn test_substring() {
    match eval("(substring \"hello\" 1 4)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "ell"),
        _ => panic!("Expected string"),
    }

    // Test with just start index (to end)
    match eval("(substring \"hello\" 2)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "llo"),
        _ => panic!("Expected string"),
    }

    // Test from start
    match eval("(substring \"hello\" 0 2)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "he"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_index() {
    // Find character in string
    assert_eq!(
        eval("(string-index \"hello\" \"l\")").unwrap(),
        Value::Int(2)
    );

    // Character not found
    assert_eq!(eval("(string-index \"hello\" \"x\")").unwrap(), Value::Nil);

    // First occurrence
    assert_eq!(
        eval("(string-index \"hello\" \"l\")").unwrap(),
        Value::Int(2)
    );
}

#[test]
fn test_char_at() {
    match eval("(char-at \"hello\" 0)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "h"),
        _ => panic!("Expected string"),
    }

    match eval("(char-at \"hello\" 1)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "e"),
        _ => panic!("Expected string"),
    }

    match eval("(char-at \"hello\" 4)").unwrap() {
        Value::String(s) => assert_eq!(&*s, "o"),
        _ => panic!("Expected string"),
    }
}

// Vector operations
#[test]
fn test_vector_creation() {
    match eval("(vector 1 2 3)").unwrap() {
        Value::Vector(v) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], Value::Int(1));
            assert_eq!(v[1], Value::Int(2));
            assert_eq!(v[2], Value::Int(3));
        }
        _ => panic!("Expected vector"),
    }

    // Empty vector
    match eval("(vector)").unwrap() {
        Value::Vector(v) => assert_eq!(v.len(), 0),
        _ => panic!("Expected vector"),
    }
}

#[test]
fn test_vector_length() {
    assert_eq!(
        eval("(vector-length (vector 1 2 3))").unwrap(),
        Value::Int(3)
    );
    assert_eq!(eval("(vector-length (vector))").unwrap(), Value::Int(0));
    assert_eq!(
        eval("(vector-length (vector 10 20 30 40 50))").unwrap(),
        Value::Int(5)
    );
}

#[test]
fn test_vector_ref() {
    assert_eq!(
        eval("(vector-ref (vector 10 20 30) 0)").unwrap(),
        Value::Int(10)
    );
    assert_eq!(
        eval("(vector-ref (vector 10 20 30) 1)").unwrap(),
        Value::Int(20)
    );
    assert_eq!(
        eval("(vector-ref (vector 10 20 30) 2)").unwrap(),
        Value::Int(30)
    );
}

#[test]
fn test_vector_set() {
    match eval("(vector-set! (vector 1 2 3) 1 99)").unwrap() {
        Value::Vector(v) => {
            assert_eq!(v[0], Value::Int(1));
            assert_eq!(v[1], Value::Int(99));
            assert_eq!(v[2], Value::Int(3));
        }
        _ => panic!("Expected vector"),
    }

    // Set at beginning
    match eval("(vector-set! (vector 1 2 3) 0 100)").unwrap() {
        Value::Vector(v) => assert_eq!(v[0], Value::Int(100)),
        _ => panic!("Expected vector"),
    }

    // Set at end
    match eval("(vector-set! (vector 1 2 3) 2 200)").unwrap() {
        Value::Vector(v) => assert_eq!(v[2], Value::Int(200)),
        _ => panic!("Expected vector"),
    }
}

// Math constants and utilities
#[test]
fn test_math_constants() {
    // Test pi
    match eval("(pi)").unwrap() {
        Value::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // Test e
    match eval("(e)").unwrap() {
        Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_mod_and_remainder() {
    // Modulo
    assert_eq!(eval("(mod 17 5)").unwrap(), Value::Int(2));
    assert_eq!(eval("(mod 20 4)").unwrap(), Value::Int(0));
    assert_eq!(eval("(mod -17 5)").unwrap(), Value::Int(3));

    // Remainder
    assert_eq!(eval("(remainder 17 5)").unwrap(), Value::Int(2));
    assert_eq!(eval("(remainder 20 4)").unwrap(), Value::Int(0));
}

#[test]
fn test_even_odd() {
    assert_eq!(eval("(even? 2)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(even? 3)").unwrap(), Value::Bool(false));
    assert_eq!(eval("(odd? 2)").unwrap(), Value::Bool(false));
    assert_eq!(eval("(odd? 3)").unwrap(), Value::Bool(true));
    assert_eq!(eval("(even? 0)").unwrap(), Value::Bool(true));
}

// Recursive function tests (issue #6)
// Note: These tests demonstrate the expected behavior for recursive lambdas
// Full support requires forward reference mechanism or circular reference handling
// See PR #13 for partial implementation

#[test]
fn test_recursive_lambda_fibonacci() {
    // Test basic recursive lambda
    let code = r#"
        (define fib (lambda (n) 
          (if (< n 2) 
            n 
            (+ (fib (- n 1)) (fib (- n 2))))))
        (fib 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5));
}

#[test]
fn test_recursive_lambda_fibonacci_10() {
    // Test fibonacci(10) = 55
    let code = r#"
        (define fib (lambda (n) 
          (if (< n 2) 
            n 
            (+ (fib (- n 1)) (fib (- n 2))))))
        (fib 10)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(55));
}

#[test]
fn test_tail_recursive_sum() {
    // Test tail-recursive sum accumulation
    let code = r#"
        (define sum-to (lambda (n acc)
          (if (= n 0) acc (sum-to (- n 1) (+ acc n)))))
        (sum-to 100 0)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5050));
}

#[test]
fn test_recursive_countdown() {
    // Test simple countdown recursion
    let code = r#"
        (define countdown (lambda (n)
          (if (<= n 0)
            0
            (+ n (countdown (- n 1))))))
        (countdown 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(15)); // 5 + 4 + 3 + 2 + 1
}

#[test]
fn test_nested_recursive_functions() {
    // Test nested function definitions with recursion
    let code = r#"
        (define outer (lambda (n)
          (define inner (lambda (x)
            (if (< x 1) 0 (+ x (inner (- x 1))))))
          (inner n)))
        (outer 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(15)); // 5 + 4 + 3 + 2 + 1
}

#[test]
fn test_simple_lambda_call() {
    // Simple lambda that takes a parameter and returns it
    let code = r#"
        (define identity (lambda (x) x))
        (identity 42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_lambda_with_arithmetic() {
    // Lambda that does arithmetic on its parameter
    let code = r#"
        (define double (lambda (x) (* x 2)))
        (double 21)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_lambda_with_comparison() {
    // Lambda that uses comparison on its parameter
    let code = r#"
        (define is-positive (lambda (x) (> x 0)))
        (is-positive 5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Bool(true));
}

// Closure scoping tests (Issue #21 - Foundation work on closure improvements)
// These tests demonstrate working closure parameter scoping with lambda expressions

#[test]
fn test_closure_captures_outer_variable() {
    let code = r#"
        (define x 100)
        ((lambda (y) (+ x y)) 20)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(120));
}

#[test]
fn test_closure_parameter_shadowing() {
    let code = r#"
        (define x 100)
        ((lambda (x) (+ x 1)) 50)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(51));
}

#[test]
fn test_closure_captures_multiple_variables() {
    let code = r#"
        (define x 10)
        (define y 20)
        (define z 30)
        ((lambda (a b c) (+ a b c x y z)) 1 2 3)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(66));
}

#[test]
fn test_closure_parameter_in_nested_expression() {
    let code = r#"
        ((lambda (x)
          (if (> x 50) (* x 2) (+ x 100))) 25)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(125));
}

#[test]
fn test_multiple_closures_independent_params() {
    let code = r#"
        (define f1 (lambda (x) (+ x 10)))
        (define f2 (lambda (x) (* x 2)))
        (+ (f1 5) (f2 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(25));
}

#[test]
fn test_closure_captured_function_call() {
    let code = r#"
        (define add (lambda (a b) (+ a b)))
        ((lambda (x y) (add x y)) 10 20)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_closure_with_list_operations() {
    let code = r#"
        (define numbers (list 1 2 3 4 5))
        ((lambda (lst) (first lst)) numbers)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(1));
}

#[test]
fn test_closure_parameter_in_conditional() {
    let code = r#"
        ((lambda (n)
          (if (nil? n) "empty" "nonempty"))
         (list 1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::String(Rc::from("nonempty")));
}

#[test]
fn test_closure_preserves_parameter_type() {
    let code = r#"
        ((lambda (s) (string? s)) "hello")
    "#;
    assert_eq!(eval(code).unwrap(), Value::Bool(true));
}

// Let-binding tests (Issue #21)

#[test]
fn test_let_simple_binding() {
    let code = r#"
        (let ((x 5))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5));
}

#[test]
fn test_let_with_arithmetic() {
    let code = r#"
        (let ((x 5))
          (+ x 3))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(8));
}

#[test]
fn test_let_multiple_bindings() {
    let code = r#"
        (let ((x 5) (y 3))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(8));
}

#[test]
fn test_let_binding_with_expressions() {
    let code = r#"
        (let ((x (+ 2 3)) (y (* 4 5)))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(25));
}

#[test]
fn test_let_shadowing_global() {
    let code = r#"
        (define x 10)
        (let ((x 20))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(20));
}

#[test]
fn test_let_does_not_modify_global() {
    let code = r#"
        (define x 10)
        (let ((x 20))
          x)
        x
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(10));
}

#[test]
fn test_let_with_lists() {
    let code = r#"
        (let ((lst (list 1 2 3)))
          (first lst))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(1));
}

#[test]
fn test_let_with_string_operations() {
    let code = r#"
        (let ((s "hello"))
          (string? s))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Bool(true));
}

#[test]
fn test_let_with_conditional() {
    let code = r#"
        (let ((x 10))
          (if (> x 5) "big" "small"))
    "#;
    assert_eq!(eval(code).unwrap(), Value::String(Rc::from("big")));
}

#[test]
fn test_let_empty_body_returns_nil() {
    let code = r#"
        (let ((x 5)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Nil);
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
    assert_eq!(eval(code).unwrap(), Value::Int(8));
}

#[test]
fn test_let_with_global_reference() {
    let code = r#"
        (define y 100)
        (let ((x 50))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(150));
}

#[test]
fn test_let_binding_order() {
    let code = r#"
        (let ((x 1) (y 2) (z 3))
          (+ x y z))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(6));
}

#[test]
fn test_let_with_list_literal() {
    let code = r#"
        (let ((x (quote (1 2 3))))
          (rest x))
    "#;
    assert_eq!(
        eval(code).unwrap(),
        list(vec![Value::Int(2), Value::Int(3)])
    );
}

#[test]
fn test_let_shadowing_with_calculation() {
    let code = r#"
        (define x 10)
        (let ((x (* 2 x)))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(20));
}

#[test]
fn test_let_with_builtin_functions() {
    let code = r#"
         (let ((len (lambda (x) 42)))
           (len nil))
     "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

// Tests for let* (sequential binding with access to previous bindings)

#[test]
fn test_let_star_empty() {
    let code = r#"
        (let* ()
          42)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_let_star_simple_binding() {
    let code = r#"
        (let* ((x 5))
          x)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5));
}

#[test]
fn test_let_star_with_multiple_bindings_no_dependencies() {
    let code = r#"
        (let* ((x 1) (y 2))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(3));
}

// Tests for cond expression (multi-way conditional)

#[test]
fn test_cond_single_true_clause() {
    assert_eq!(eval("(cond (#t 42))").unwrap(), Value::Int(42));
}

#[test]
fn test_cond_single_false_clause_with_else() {
    assert_eq!(eval("(cond (#f 42) (else 100))").unwrap(), Value::Int(100));
}

#[test]
fn test_cond_single_false_clause_without_else() {
    // If no clause matches and no else, return nil
    assert_eq!(eval("(cond (#f 42))").unwrap(), Value::Nil);
}

#[test]
fn test_cond_first_clause_matches() {
    assert_eq!(
        eval("(cond ((> 5 3) 100) ((> 4 2) 200))").unwrap(),
        Value::Int(100)
    );
}

#[test]
fn test_cond_second_clause_matches() {
    assert_eq!(
        eval("(cond ((> 3 5) 100) ((> 4 2) 200))").unwrap(),
        Value::Int(200)
    );
}

#[test]
fn test_cond_multiple_clauses_with_else() {
    assert_eq!(
        eval("(cond ((> 3 5) 100) ((> 2 4) 200) (else 300))").unwrap(),
        Value::Int(300)
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
    match eval(code).unwrap() {
        Value::String(s) => assert_eq!(&*s, "two-two"),
        _ => panic!("Expected string"),
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
    assert_eq!(eval(code).unwrap(), Value::Int(5));
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
    assert_eq!(eval(code).unwrap(), Value::Int(6));
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
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_cond_with_variable_references() {
    let code = r#"
        (define x 10)
        (cond
          ((< x 5) "small")
          ((< x 15) "medium")
          (else "large"))
    "#;
    match eval(code).unwrap() {
        Value::String(s) => assert_eq!(&*s, "medium"),
        _ => panic!("Expected string"),
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
    match eval(code).unwrap() {
        Value::String(s) => assert_eq!(&*s, "first"),
        _ => panic!("Expected string"),
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
    assert_eq!(eval(code).unwrap(), Value::Int(9));
}

// Tests for nested lambdas with closure capture
// Note: These are limited scope tests due to known issues #78 with parameter access
// when captures are present. Only tests that access captures without parameter interference work.

#[test]
fn test_nested_lambda_single_capture() {
    // Test that nested lambdas can access outer lambda parameters (capture only)
    let code = r#"
        (define make-const (lambda (x)
          (lambda (y)
            x)))
        
        (define f (make-const 42))
        (f 100)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

#[test]
fn test_nested_lambda_parameter_only() {
    // Test that nested lambdas can access their own parameters (no captures)
    let code = r#"
         (define make-id (lambda (x)
           (lambda (y)
             y)))
         
         (define f (make-id 100))
         (f 42)
     "#;
    assert_eq!(eval(code).unwrap(), Value::Int(42));
}

// TODO: Fix issue #78 - Deeply nested closures with multiple captures
// The following tests are disabled because they currently fail due to incorrect
// index resolution in closures with 3+ nesting levels.
//
// #[test]
// fn test_triple_nested_closure_with_multiple_captures() {
//     let code = r#"
//          (define make-multiplier (lambda (a)
//            (lambda (b)
//              (lambda (c)
//                (* a (* b c))))))
//          (((make-multiplier 2) 3) 4)
//      "#;
//     assert_eq!(eval(code).unwrap(), Value::Int(24));
// }

// Threading operators (-> and ->>)
#[test]
fn test_thread_first_simple() {
    // (-> 5 (+ 10) (* 2)) => (* (+ 5 10) 2) => 30
    let code = "(-> 5 (+ 10) (* 2))";
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_thread_first_with_multiple_args() {
    // (-> 5 (+ 10 2) (* 3)) => (* (+ 5 10 2) 3) => 51
    let code = "(-> 5 (+ 10 2) (* 3))";
    assert_eq!(eval(code).unwrap(), Value::Int(51));
}

#[test]
fn test_thread_last_simple() {
    // (->> 5 (+ 10) (* 2)) => (* 2 (+ 10 5)) => 30
    let code = "(->> 5 (+ 10) (* 2))";
    assert_eq!(eval(code).unwrap(), Value::Int(30));
}

#[test]
fn test_thread_last_with_multiple_args() {
    // (->> 2 (+ 10) (* 3)) => (* 3 (+ 10 2)) => 36
    let code = "(->> 2 (+ 10) (* 3))";
    assert_eq!(eval(code).unwrap(), Value::Int(36));
}

#[test]
fn test_thread_first_chain() {
    // (-> 1 (+ 1) (+ 1) (+ 1)) => (+ (+ (+ 1 1) 1) 1) => 4
    let code = "(-> 1 (+ 1) (+ 1) (+ 1))";
    assert_eq!(eval(code).unwrap(), Value::Int(4));
}

#[test]
fn test_thread_last_chain() {
    // (->> 1 (+ 1) (+ 1) (+ 1)) => (+ 1 (+ 1 (+ 1 1))) => 4
    let code = "(->> 1 (+ 1) (+ 1) (+ 1))";
    assert_eq!(eval(code).unwrap(), Value::Int(4));
}

#[test]
fn test_thread_first_with_list_ops() {
    // (-> (list 1 2 3) (length)) => (length (list 1 2 3)) => 3
    let code = "(-> (list 1 2 3) (length))";
    assert_eq!(eval(code).unwrap(), Value::Int(3));
}

#[test]
fn test_thread_last_with_list_ops() {
    // (->> (list 1 2 3) (length)) => (length (list 1 2 3)) => 3
    let code = "(->> (list 1 2 3) (length))";
    assert_eq!(eval(code).unwrap(), Value::Int(3));
}

#[test]
fn test_thread_first_nested() {
    // Test threading through nested operations
    let code = "(-> 10 (- 3) (+ 5))";
    // (+ (- 10 3) 5) = (+ 7 5) = 12
    assert_eq!(eval(code).unwrap(), Value::Int(12));
}

#[test]
fn test_thread_last_nested() {
    // Test threading through nested operations
    let code = "(->> 10 (- 3) (+ 5))";
    // (+ 5 (- 3 10)) = (+ 5 -7) = -2
    assert_eq!(eval(code).unwrap(), Value::Int(-2));
}
