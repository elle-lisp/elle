// DEFENSE: Reader must handle all valid Lisp syntax correctly
use elle::reader::read_str;
use elle::symbol::SymbolTable;
use elle::value::Value;

#[test]
fn test_read_integers() {
    let mut symbols = SymbolTable::new();

    assert_eq!(read_str("0", &mut symbols).unwrap(), Value::int(0));
    assert_eq!(read_str("42", &mut symbols).unwrap(), Value::int(42));
    assert_eq!(read_str("-123", &mut symbols).unwrap(), Value::int(-123));
    assert_eq!(read_str("+456", &mut symbols).unwrap(), Value::int(456));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_read_floats() {
    let mut symbols = SymbolTable::new();

    assert_eq!(read_str("3.14", &mut symbols).unwrap(), Value::float(3.14));
    assert_eq!(read_str("-2.5", &mut symbols).unwrap(), Value::float(-2.5));
    assert_eq!(read_str("0.0", &mut symbols).unwrap(), Value::float(0.0));
}

#[test]
fn test_read_booleans() {
    let mut symbols = SymbolTable::new();

    assert_eq!(read_str("#t", &mut symbols).unwrap(), Value::bool(true));
    assert_eq!(read_str("#f", &mut symbols).unwrap(), Value::bool(false));
}

#[test]
fn test_read_nil() {
    let mut symbols = SymbolTable::new();
    assert_eq!(read_str("nil", &mut symbols).unwrap(), Value::NIL);
}

#[test]
fn test_read_symbols() {
    let mut symbols = SymbolTable::new();

    let sym = read_str("foo", &mut symbols).unwrap();
    assert!((sym).is_symbol());

    let sym2 = read_str("bar-baz", &mut symbols).unwrap();
    assert!((sym2).is_symbol());

    let sym3 = read_str("+", &mut symbols).unwrap();
    assert!((sym3).is_symbol());
}

#[test]
fn test_read_strings() {
    let mut symbols = SymbolTable::new();

    if let Some(s) = read_str("\"hello\"", &mut symbols).unwrap().as_string() {
        assert_eq!(s, "hello");
    } else {
        panic!("Expected string");
    }

    if let Some(s) = read_str("\"\"", &mut symbols).unwrap().as_string() {
        assert_eq!(s, "");
    } else {
        panic!("Expected empty string");
    }
}

#[test]
fn test_read_string_escapes() {
    let mut symbols = SymbolTable::new();

    if let Some(s) = read_str(r#""hello\nworld""#, &mut symbols)
        .unwrap()
        .as_string()
    {
        assert_eq!(s, "hello\nworld");
    } else {
        panic!("Expected string");
    }

    if let Some(s) = read_str(r#""quote: \"test\"""#, &mut symbols)
        .unwrap()
        .as_string()
    {
        assert_eq!(s, "quote: \"test\"");
    } else {
        panic!("Expected string");
    }
}

#[test]
fn test_read_empty_list() {
    let mut symbols = SymbolTable::new();

    let result = read_str("()", &mut symbols).unwrap();
    // Empty list is NOT nil - they are distinct values
    assert_eq!(result, Value::EMPTY_LIST);
    assert!(result.is_empty_list());
    assert!(!result.is_nil());
}

#[test]
fn test_read_simple_list() {
    let mut symbols = SymbolTable::new();

    let result = read_str("(1 2 3)", &mut symbols).unwrap();
    assert!(result.is_list());

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_read_nested_lists() {
    let mut symbols = SymbolTable::new();

    let result = read_str("((1 2) (3 4))", &mut symbols).unwrap();
    assert!(result.is_list());

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert!(vec[0].is_list());
    assert!(vec[1].is_list());
}

#[test]
fn test_read_quote() {
    let mut symbols = SymbolTable::new();

    let result = read_str("'foo", &mut symbols).unwrap();
    assert!(result.is_list());

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    // First element should be 'quote symbol
    assert!((vec[0]).is_symbol());
}

#[test]
fn test_read_quasiquote() {
    let mut symbols = SymbolTable::new();

    let result = read_str("`foo", &mut symbols).unwrap();
    assert!(result.is_list());

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_read_unquote() {
    let mut symbols = SymbolTable::new();

    let result = read_str(",foo", &mut symbols).unwrap();
    assert!(result.is_list());
}

#[test]
fn test_read_vector() {
    let mut symbols = SymbolTable::new();

    let result = read_str("[1 2 3]", &mut symbols).unwrap();
    if let Some(v) = result.as_vector() {
        let v_ref = v.borrow();
        assert_eq!(v_ref.len(), 3);
        assert_eq!(v_ref[0], Value::int(1));
        assert_eq!(v_ref[1], Value::int(2));
        assert_eq!(v_ref[2], Value::int(3));
    } else {
        panic!("Expected vector");
    }
}

#[test]
fn test_read_empty_vector() {
    let mut symbols = SymbolTable::new();

    let result = read_str("[]", &mut symbols).unwrap();
    if let Some(v) = result.as_vector() {
        assert_eq!(v.borrow().len(), 0);
    } else {
        panic!("Expected vector");
    }
}

#[test]
fn test_read_comments() {
    let mut symbols = SymbolTable::new();

    // Comment should be ignored
    let result = read_str("; this is a comment\n42", &mut symbols).unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_read_whitespace() {
    let mut symbols = SymbolTable::new();

    // Various whitespace should be handled
    let result = read_str("  \t\n  42  \t\n  ", &mut symbols).unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_read_complex_expression() {
    let mut symbols = SymbolTable::new();

    let result = read_str("(+ (* 2 3) (- 10 5))", &mut symbols).unwrap();
    assert!(result.is_list());

    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3); // +, first arg, second arg
}

#[test]
fn test_read_errors() {
    let mut symbols = SymbolTable::new();

    // Unterminated string
    assert!(read_str("\"hello", &mut symbols).is_err());

    // Unterminated list
    assert!(read_str("(1 2 3", &mut symbols).is_err());

    // Unexpected closing paren
    assert!(read_str(")", &mut symbols).is_err());

    // Note: "12.34.56" is currently parsed as a symbol, not rejected as invalid
    // This is a known limitation - the lexer doesn't validate number format strictly
}

#[test]
fn test_read_special_symbols() {
    let mut symbols = SymbolTable::new();

    // Arithmetic operators
    for op in &["+", "-", "*", "/", "=", "<", ">"] {
        let result = read_str(op, &mut symbols).unwrap();
        assert!((result).is_symbol());
    }

    // Hyphenated names (common in Lisp)
    let result = read_str("some-func-name", &mut symbols).unwrap();
    assert!((result).is_symbol());
}

#[test]
fn test_read_deep_nesting() {
    let mut symbols = SymbolTable::new();

    // Test deep nesting doesn't cause stack overflow
    let result = read_str("((((((((((42))))))))))", &mut symbols).unwrap();
    assert!(result.is_list());
}

#[test]
fn test_read_large_list() {
    let mut symbols = SymbolTable::new();

    // Generate a list with 100 elements
    let input = format!(
        "({})",
        (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let result = read_str(&input, &mut symbols).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 100);
}

#[test]
fn test_symbol_interning() {
    let mut symbols = SymbolTable::new();

    // Same symbol should have same ID
    let sym1 = read_str("foo", &mut symbols).unwrap();
    let sym2 = read_str("foo", &mut symbols).unwrap();
    assert_eq!(sym1, sym2);

    // Different symbols should have different IDs
    let sym3 = read_str("bar", &mut symbols).unwrap();
    assert_ne!(sym1, sym3);
}

#[test]
fn test_comments() {
    let mut symbols = SymbolTable::new();

    // Single line comment
    let result = read_str("42 ; this is a comment", &mut symbols);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));

    // Comment with nothing after
    let result = read_str("42 ;", &mut symbols);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));

    // Multiple statements with comments
    let result = read_str("(+ 1 2) ; add", &mut symbols);
    assert!(result.is_ok());
    if !result.unwrap().is_cons() {
        panic!("Expected list");
    }

    // Comment between elements
    let result = read_str("(+ 1 ; first\n 2)", &mut symbols);
    assert!(result.is_ok());
    if !result.unwrap().is_cons() {
        panic!("Expected list");
    }

    // Empty with comment
    let result = read_str("; just a comment", &mut symbols);
    assert!(result.is_err()); // No expression to read
}
