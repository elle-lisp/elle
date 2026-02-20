//! Tests for NaN-boxed Value representation.

use super::*;

#[test]
fn test_size() {
    assert_eq!(std::mem::size_of::<Value>(), 8);
}

#[test]
fn test_nil() {
    let v = Value::NIL;
    assert!(v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_truthy()); // nil is falsy
}

#[test]
fn test_undefined() {
    let v = Value::UNDEFINED;
    assert!(v.is_undefined());
    assert!(!v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_empty_list());
    // Note: is_truthy() now debug_asserts that UNDEFINED never reaches it.
    // UNDEFINED should never appear in user-visible evaluation.

    // Verify UNDEFINED is distinct from all other special constants
    assert_ne!(Value::UNDEFINED.to_bits(), Value::NIL.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::TRUE.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::FALSE.to_bits());
    assert_ne!(Value::UNDEFINED.to_bits(), Value::EMPTY_LIST.to_bits());
}

#[test]
fn test_bool() {
    assert!(Value::TRUE.is_bool());
    assert!(Value::FALSE.is_bool());
    assert_eq!(Value::TRUE.as_bool(), Some(true));
    assert_eq!(Value::FALSE.as_bool(), Some(false));
    assert!(Value::TRUE.is_truthy());
    assert!(!Value::FALSE.is_truthy());
}

#[test]
fn test_int_roundtrip() {
    for &n in &[0i64, 1, -1, 100, -100, INT_MAX, INT_MIN] {
        let v = Value::int(n);
        assert!(v.is_int());
        assert!(!v.is_float());
        assert_eq!(v.as_int(), Some(n), "Failed for {}", n);
    }
}

#[test]
fn test_float_roundtrip() {
    for &f in &[
        0.0f64,
        1.0,
        -1.0,
        std::f64::consts::PI,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let v = Value::float(f);
        assert!(v.is_float());
        assert!(!v.is_int());
        assert_eq!(v.as_float(), Some(f));
    }
}

#[test]
fn test_symbol() {
    let v = Value::symbol(42);
    assert!(v.is_symbol());
    assert_eq!(v.as_symbol(), Some(42));
}

#[test]
fn test_keyword() {
    let v = Value::keyword(123);
    assert!(v.is_keyword());
    assert_eq!(v.as_keyword(), Some(123));
}

#[test]
fn test_bool_constructor() {
    assert_eq!(Value::bool(true), Value::TRUE);
    assert_eq!(Value::bool(false), Value::FALSE);
}

#[test]
fn test_string_constructor() {
    let v = Value::string("hello");
    assert!(v.is_string());
    assert_eq!(v.as_string(), Some("hello"));
}

#[test]
fn test_cons_constructor() {
    let car = Value::int(1);
    let cdr = Value::int(2);
    let v = Value::cons(car, cdr);
    assert!(v.is_cons());
    if let Some(cons) = v.as_cons() {
        assert_eq!(cons.first, car);
        assert_eq!(cons.rest, cdr);
    } else {
        panic!("Expected cons cell");
    }
}

#[test]
fn test_vector_constructor() {
    let elements = vec![Value::int(1), Value::int(2), Value::int(3)];
    let v = Value::vector(elements.clone());
    assert!(v.is_vector());
    if let Some(vec_ref) = v.as_vector() {
        let borrowed = vec_ref.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0], Value::int(1));
        assert_eq!(borrowed[1], Value::int(2));
        assert_eq!(borrowed[2], Value::int(3));
    } else {
        panic!("Expected vector");
    }
}

#[test]
fn test_table_constructor() {
    let v = Value::table();
    assert!(v.is_table());
    if let Some(table_ref) = v.as_table() {
        let borrowed = table_ref.borrow();
        assert_eq!(borrowed.len(), 0);
    } else {
        panic!("Expected table");
    }
}

#[test]
fn test_cell_constructor() {
    let inner = Value::int(42);
    let v = Value::cell(inner);
    assert!(v.is_cell());
    if let Some(cell_ref) = v.as_cell() {
        let borrowed = cell_ref.borrow();
        assert_eq!(*borrowed, Value::int(42));
    } else {
        panic!("Expected cell");
    }
}

#[test]
fn test_list_function() {
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let list_val = list(values);
    assert!(list_val.is_list());

    // Convert back to vec
    let result = list_val.list_to_vec().unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], Value::int(1));
    assert_eq!(result[1], Value::int(2));
    assert_eq!(result[2], Value::int(3));
}

#[test]
fn test_is_list() {
    // Proper list
    let proper_list = Value::cons(Value::int(1), Value::cons(Value::int(2), Value::NIL));
    assert!(proper_list.is_list());

    // Not a list (improper list)
    let improper_list = Value::cons(Value::int(1), Value::int(2));
    assert!(!improper_list.is_list());

    // Nil is a list
    assert!(Value::NIL.is_list());
}

#[test]
fn test_type_name() {
    assert_eq!(Value::NIL.type_name(), "nil");
    assert_eq!(Value::TRUE.type_name(), "boolean");
    assert_eq!(Value::int(42).type_name(), "integer");
    assert_eq!(Value::float(std::f64::consts::PI).type_name(), "float");
    assert_eq!(Value::symbol(1).type_name(), "symbol");
    assert_eq!(Value::keyword(1).type_name(), "keyword");
    assert_eq!(Value::string("test").type_name(), "string");
    assert_eq!(
        Value::cons(Value::NIL, Value::EMPTY_LIST).type_name(),
        "cons"
    );
    assert_eq!(Value::vector(vec![]).type_name(), "vector");
    assert_eq!(Value::table().type_name(), "table");
    assert_eq!(Value::cell(Value::NIL).type_name(), "cell");
}

#[test]
fn test_truthiness_semantics() {
    // Only nil and #f are falsy
    assert!(!Value::NIL.is_truthy(), "nil is falsy");
    assert!(!Value::FALSE.is_truthy(), "#f is falsy");

    // #t is truthy
    assert!(Value::TRUE.is_truthy(), "#t is truthy");

    // Zero is truthy (not falsy like in C)
    assert!(Value::int(0).is_truthy(), "0 is truthy");
    assert!(Value::float(0.0).is_truthy(), "0.0 is truthy");

    // Empty string is truthy
    assert!(Value::string("").is_truthy(), "empty string is truthy");

    // Empty list is truthy (it's nil, but we test the list form)
    assert!(Value::EMPTY_LIST.is_truthy(), "empty list is truthy");

    // Empty vector is truthy
    assert!(Value::vector(vec![]).is_truthy(), "empty vector is truthy");

    // Regular values are truthy
    assert!(Value::int(1).is_truthy(), "1 is truthy");
    assert!(Value::int(-1).is_truthy(), "-1 is truthy");
    assert!(
        Value::float(std::f64::consts::PI).is_truthy(),
        "PI is truthy"
    );
    assert!(
        Value::string("hello").is_truthy(),
        "non-empty string is truthy"
    );
    assert!(Value::symbol(1).is_truthy(), "symbol is truthy");
    assert!(Value::keyword(1).is_truthy(), "keyword is truthy");

    // Non-empty list is truthy
    let non_empty_list = Value::cons(Value::int(1), Value::NIL);
    assert!(non_empty_list.is_truthy(), "non-empty list is truthy");

    // Non-empty vector is truthy
    let non_empty_vec = Value::vector(vec![Value::int(1)]);
    assert!(non_empty_vec.is_truthy(), "non-empty vector is truthy");

    // Table is truthy
    assert!(Value::table().is_truthy(), "table is truthy");

    // Cell is truthy
    assert!(Value::cell(Value::int(42)).is_truthy(), "cell is truthy");
}
