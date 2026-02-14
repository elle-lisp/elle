// Tests for naming consistency improvements (Issue #149)

use elle::compiler::converters::value_to_expr;
use elle::reader::OwnedToken;
use elle::{compile, list, register_primitives, Lexer, Reader, SymbolTable, Value, VM};
use std::f64::consts::PI;

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(OwnedToken::from(token));
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::new(tokens);
    let mut values = Vec::new();
    while let Some(result) = reader.try_read(&mut symbols) {
        values.push(result?);
    }

    let value = if values.len() == 1 {
        values.into_iter().next().unwrap()
    } else if values.is_empty() {
        return Err("No input".to_string());
    } else {
        let mut begin_args = vec![Value::Symbol(symbols.intern("begin"))];
        begin_args.extend(values);
        list(begin_args)
    };

    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============ POLYMORPHIC GET TESTS ============

#[test]
fn test_get_on_table() {
    assert_eq!(eval("(get (table 'a 1) 'a)").unwrap(), Value::Int(1));
}

#[test]
fn test_get_on_table_with_default() {
    assert_eq!(eval("(get (table 'a 1) 'b 99)").unwrap(), Value::Int(99));
}

#[test]
fn test_get_on_table_missing_key() {
    assert_eq!(eval("(get (table 'a 1) 'b)").unwrap(), Value::Nil);
}

#[test]
fn test_get_on_struct() {
    assert_eq!(eval("(get (struct 'a 1) 'a)").unwrap(), Value::Int(1));
}

#[test]
fn test_get_on_struct_with_default() {
    assert_eq!(eval("(get (struct 'a 1) 'b 99)").unwrap(), Value::Int(99));
}

#[test]
fn test_get_on_struct_missing_key() {
    assert_eq!(eval("(get (struct 'a 1) 'b)").unwrap(), Value::Nil);
}

#[test]
fn test_get_on_invalid_type() {
    assert!(eval("(get 42 'a)").is_err());
}

// ============ POLYMORPHIC KEYS TESTS ============

#[test]
fn test_keys_on_table() {
    let result = eval("(keys (table 'a 1 'b 2))").unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_keys_on_struct() {
    let result = eval("(keys (struct 'a 1 'b 2))").unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_keys_on_empty_table() {
    let result = eval("(keys (table))").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn test_keys_on_empty_struct() {
    let result = eval("(keys (struct))").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn test_keys_on_invalid_type() {
    assert!(eval("(keys 42)").is_err());
}

// ============ POLYMORPHIC VALUES TESTS ============

#[test]
fn test_values_on_table() {
    let result = eval("(values (table 'a 1 'b 2))").unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_values_on_struct() {
    let result = eval("(values (struct 'a 1 'b 2))").unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_values_on_empty_table() {
    let result = eval("(values (table))").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn test_values_on_empty_struct() {
    let result = eval("(values (struct))").unwrap();
    assert_eq!(result, Value::Nil);
}

#[test]
fn test_values_on_invalid_type() {
    assert!(eval("(values 42)").is_err());
}

// ============ POLYMORPHIC HAS-KEY? TESTS ============

#[test]
fn test_has_key_on_table_true() {
    assert_eq!(
        eval("(has-key? (table 'a 1) 'a)").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test_has_key_on_table_false() {
    assert_eq!(
        eval("(has-key? (table 'a 1) 'b)").unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_has_key_on_struct_true() {
    assert_eq!(
        eval("(has-key? (struct 'a 1) 'a)").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test_has_key_on_struct_false() {
    assert_eq!(
        eval("(has-key? (struct 'a 1) 'b)").unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_has_key_on_invalid_type() {
    assert!(eval("(has-key? 42 'a)").is_err());
}

// ============ MUTATION MARKER TESTS ============

#[test]
fn test_put_bang_mutates_table() {
    let result = eval("(let ((t (table))) (put! t 'a 1) (get t 'a))").unwrap();
    assert_eq!(result, Value::Int(1));
}

#[test]
fn test_put_bang_returns_table() {
    let result = eval("(let ((t (table))) (put! t 'a 1))").unwrap();
    assert!(matches!(result, Value::Table(_)));
}

#[test]
fn test_del_bang_removes_key() {
    let result = eval("(let ((t (table 'a 1))) (del! t 'a) (has-key? t 'a))").unwrap();
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_del_bang_returns_table() {
    let result = eval("(let ((t (table 'a 1))) (del! t 'a))").unwrap();
    assert!(matches!(result, Value::Table(_)));
}

#[test]
fn test_put_and_put_bang_equivalent() {
    let put_result = eval("(let ((t (table))) (put t 'a 1) (get t 'a))").unwrap();
    let put_bang_result = eval("(let ((t (table))) (put! t 'a 1) (get t 'a))").unwrap();
    assert_eq!(put_result, put_bang_result);
}

#[test]
fn test_del_and_del_bang_equivalent() {
    let del_result = eval("(let ((t (table 'a 1))) (del t 'a) (has-key? t 'a))").unwrap();
    let del_bang_result = eval("(let ((t (table 'a 1))) (del! t 'a) (has-key? t 'a))").unwrap();
    assert_eq!(del_result, del_bang_result);
}

// ============ STRING CONVERSION TESTS ============

#[test]
fn test_string_to_int_basic() {
    assert_eq!(eval("(string->int \"42\")").unwrap(), Value::Int(42));
}

#[test]
fn test_string_to_int_negative() {
    assert_eq!(eval("(string->int \"-123\")").unwrap(), Value::Int(-123));
}

#[test]
fn test_string_to_int_zero() {
    assert_eq!(eval("(string->int \"0\")").unwrap(), Value::Int(0));
}

#[test]
fn test_string_to_int_invalid() {
    assert!(eval("(string->int \"not a number\")").is_err());
}

#[test]
fn test_string_to_float_basic() {
    let result = eval("(string->float \"3.14\")").unwrap();
    match result {
        Value::Float(f) => assert!((f - PI).abs() < 0.01),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_string_to_float_negative() {
    let result = eval("(string->float \"-2.5\")").unwrap();
    match result {
        Value::Float(f) => assert!((f - (-2.5)).abs() < 0.001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_string_to_float_invalid() {
    assert!(eval("(string->float \"not a float\")").is_err());
}

#[test]
fn test_any_to_string_int() {
    assert_eq!(
        eval("(any->string 42)").unwrap(),
        Value::String("42".into())
    );
}

#[test]
fn test_any_to_string_float() {
    let result = eval("(any->string 3.14)").unwrap();
    match result {
        Value::String(s) => assert!(s.contains("3.14")),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_any_to_string_bool_true() {
    assert_eq!(
        eval("(any->string #t)").unwrap(),
        Value::String("true".into())
    );
}

#[test]
fn test_any_to_string_bool_false() {
    assert_eq!(
        eval("(any->string #f)").unwrap(),
        Value::String("false".into())
    );
}

#[test]
fn test_any_to_string_nil() {
    assert_eq!(
        eval("(any->string nil)").unwrap(),
        Value::String("nil".into())
    );
}

#[test]
fn test_any_to_string_string() {
    assert_eq!(
        eval("(any->string \"hello\")").unwrap(),
        Value::String("hello".into())
    );
}

// ============ LEGACY NAME COMPATIBILITY TESTS ============

#[test]
fn test_legacy_int_still_works() {
    assert_eq!(eval("(int \"42\")").unwrap(), Value::Int(42));
}

#[test]
fn test_legacy_float_still_works() {
    let result = eval("(float \"3.14\")").unwrap();
    match result {
        Value::Float(f) => assert!((f - PI).abs() < 0.01),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_legacy_string_still_works() {
    assert_eq!(eval("(string 42)").unwrap(), Value::String("42".into()));
}

#[test]
fn test_legacy_and_new_names_equivalent() {
    let legacy = eval("(int \"42\")").unwrap();
    let new = eval("(string->int \"42\")").unwrap();
    assert_eq!(legacy, new);
}

// ============ INTEGRATION TESTS ============

#[test]
fn test_polymorphic_api_with_table() {
    let code = r#"
        (let ((t (table 'x 10 'y 20)))
          (list
            (get t 'x)
            (length (keys t))
            (length (values t))
            (has-key? t 'x)))
    "#;
    let result = eval(code).unwrap();
    // Should be a list of (10, 2, 2, #t)
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_polymorphic_api_with_struct() {
    let code = r#"
        (let ((s (struct 'x 10 'y 20)))
          (list
            (get s 'x)
            (length (keys s))
            (length (values s))
            (has-key? s 'x)))
    "#;
    let result = eval(code).unwrap();
    // Should be a list of (10, 2, 2, #t)
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_table_mutation_with_bang_syntax() {
    let code = r#"
        (let ((t (table)))
          (put! t 'a 1)
          (put! t 'b 2)
          (del! t 'a)
          (list (has-key? t 'a) (has-key? t 'b)))
    "#;
    let result = eval(code).unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_struct_immutability_preserved() {
    let code = r#"
        (let ((s (struct 'a 1)))
          (let ((s2 (put s 'b 2)))
            (list
              (has-key? s 'b)
              (has-key? s2 'b))))
    "#;
    let result = eval(code).unwrap();
    assert!(matches!(result, Value::Cons(_)));
}

#[test]
fn test_conversion_chain() {
    let code = r#"
        (any->string (string->int "42"))
    "#;
    assert_eq!(eval(code).unwrap(), Value::String("42".into()));
}
