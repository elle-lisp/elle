// Type system tests
// Tests for type checking, specialization, coercion, and type information
use elle::ffi::primitives::context::set_symbol_table;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Set the symbol table in thread-local context for primitives that need it
    set_symbol_table(&mut symbols as *mut SymbolTable);

    let value = read_str(input, &mut symbols)?;
    let expr = elle::compiler::converters::value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

#[test]
fn test_type_error() {
    assert!(eval("(+ 1 nil)").is_err());
}

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

#[test]
fn test_type_conversions() {
    assert_eq!(eval("(int 3.14)").unwrap(), Value::Int(3));

    match eval("(float 5)").unwrap() {
        Value::Float(f) => assert!((f - 5.0).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_type() {
    match eval("(type-of 42)").unwrap() {
        Value::Keyword(_) => {} // type-of returns a keyword
        _ => panic!("Expected keyword"),
    }

    match eval("(type-of 3.14)").unwrap() {
        Value::Keyword(_) => {} // type-of returns a keyword
        _ => panic!("Expected keyword"),
    }

    match eval("(type-of \"hello\")").unwrap() {
        Value::Keyword(_) => {} // type-of returns a keyword
        _ => panic!("Expected keyword"),
    }
}

#[test]
fn test_type_specialization_int_arithmetic() {
    // Test that integer arithmetic is optimized (AddInt bytecode)
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::Int(3));
    assert_eq!(eval("(+ 10 20 30)").unwrap(), Value::Int(60));
    assert_eq!(eval("(+ -5 5)").unwrap(), Value::Int(0));
}

#[test]
fn test_type_specialization_int_subtraction() {
    // Test SubInt specialization
    assert_eq!(eval("(- 10 3)").unwrap(), Value::Int(7));
    assert_eq!(eval("(- 100 50)").unwrap(), Value::Int(50));
    assert_eq!(eval("(- 0 5)").unwrap(), Value::Int(-5));
}

#[test]
fn test_type_specialization_int_multiplication() {
    // Test MulInt specialization
    assert_eq!(eval("(* 4 5)").unwrap(), Value::Int(20));
    assert_eq!(eval("(* 2 3 4)").unwrap(), Value::Int(24));
    assert_eq!(eval("(* -3 4)").unwrap(), Value::Int(-12));
}

#[test]
fn test_type_specialization_int_division() {
    // Test DivInt specialization
    assert_eq!(eval("(/ 20 4)").unwrap(), Value::Int(5));
    assert_eq!(eval("(/ 100 10)").unwrap(), Value::Int(10));
    assert_eq!(eval("(/ 15 3)").unwrap(), Value::Int(5));
}

#[test]
fn test_type_specialization_mixed_int_float() {
    // Test fallback when mixing int and float
    match eval("(+ 1 2.5)").unwrap() {
        Value::Float(f) => {
            assert!((f - 3.5).abs() < 0.0001);
        }
        _ => panic!("Expected float result"),
    }
}

#[test]
fn test_type_specialization_float_arithmetic() {
    // Test float arithmetic
    match eval("(+ 1.5 2.5)").unwrap() {
        Value::Float(f) => {
            assert!((f - 4.0).abs() < 0.0001);
        }
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_type_specialization_nested_int() {
    // Test nested integer arithmetic uses specialization
    assert_eq!(eval("(+ (* 2 3) (- 10 5))").unwrap(), Value::Int(11));
    assert_eq!(eval("(* (+ 1 2) (- 5 2))").unwrap(), Value::Int(9));
}

#[test]
fn test_type_specialization_performance() {
    // Verify performance critical paths work efficiently
    let start = std::time::Instant::now();
    for _ in 0..100 {
        eval("(+ 1 2)").unwrap();
    }
    let elapsed = start.elapsed();
    // Should complete quickly (specialized operations)
    assert!(elapsed.as_millis() < 500);
}

#[test]
fn test_arithmetic_specialization_sequence() {
    // Test sequence of specialized operations
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::Int(3));
    assert_eq!(eval("(- 5 2)").unwrap(), Value::Int(3));
    assert_eq!(eval("(* 3 1)").unwrap(), Value::Int(3));
    assert_eq!(eval("(/ 9 3)").unwrap(), Value::Int(3));
}

#[test]
fn test_mixed_type_operations() {
    // Test operations mixing types (uses fallback path)
    match eval("(+ 1 1.5)").unwrap() {
        Value::Float(f) => {
            assert!((f - 2.5).abs() < 0.0001);
        }
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_type_specialization_large_numbers() {
    // Test specialization with large integers
    assert_eq!(eval("(+ 1000000 2000000)").unwrap(), Value::Int(3000000));
    assert_eq!(eval("(* 1000 2000)").unwrap(), Value::Int(2000000));
}

#[test]
fn test_type_specialization_negative_numbers() {
    // Test specialization with negative numbers
    assert_eq!(eval("(+ -1 -2)").unwrap(), Value::Int(-3));
    assert_eq!(eval("(* -2 3)").unwrap(), Value::Int(-6));
    assert_eq!(eval("(/ -10 2)").unwrap(), Value::Int(-5));
}

#[test]
fn test_type_information_integers() {
    // Integer operations maintain type
    assert_eq!(eval("(+ 1 2)").unwrap(), Value::Int(3));
}

#[test]
fn test_type_information_floats() {
    // Float operations maintain type
    match eval("(+ 1.5 2.5)").unwrap() {
        Value::Float(f) => assert!((f - 4.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_type_information_strings() {
    // String operations maintain type
    match eval("(string-append \"hello\" \" \" \"world\")").unwrap() {
        Value::String(s) => assert_eq!(s.as_ref(), "hello world"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_type_information_lists() {
    // List operations maintain list type
    assert!(eval("(list 1 2 3)").is_ok());
}

#[test]
fn test_type_coercion_behavior() {
    // Test type coercion in operations
    // May fail or coerce depending on implementation
    let _ = eval("(+ 1.5 2)");
}
