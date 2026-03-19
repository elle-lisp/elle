// Unit tests for tagged-union Value representation.
//
// These tests verify the fundamental invariants of the Value type:
// roundtrip fidelity, type discrimination, truthiness, and equality.
// Converted from property tests to deterministic unit tests with concrete cases.

use elle::Value;

// =========================================================================
// Integer roundtrip
// =========================================================================

#[test]
fn int_roundtrip_min() {
    let v = Value::int(i64::MIN);
    assert_eq!(v.as_int(), Some(i64::MIN));
}

#[test]
fn int_roundtrip_max() {
    let v = Value::int(i64::MAX);
    assert_eq!(v.as_int(), Some(i64::MAX));
}

#[test]
fn int_roundtrip_zero() {
    let v = Value::int(0);
    assert_eq!(v.as_int(), Some(0));
}

#[test]
fn int_roundtrip_one() {
    let v = Value::int(1);
    assert_eq!(v.as_int(), Some(1));
}

#[test]
fn int_roundtrip_neg_one() {
    let v = Value::int(-1);
    assert_eq!(v.as_int(), Some(-1));
}

#[test]
fn int_is_int_type() {
    let v = Value::int(42);
    assert!(v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_symbol());
    assert!(!v.is_keyword());
    assert!(!v.is_heap());
    assert!(!v.is_empty_list());
}

// =========================================================================
// Float roundtrip
// =========================================================================

#[test]
fn float_roundtrip_zero() {
    let v = Value::float(0.0);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some(0.0f64.to_bits()));
}

#[test]
fn float_roundtrip_neg_zero() {
    let v = Value::float(-0.0);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some((-0.0f64).to_bits()));
}

#[test]
fn float_roundtrip_positive() {
    let v = Value::float(1.5);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some(1.5f64.to_bits()));
}

#[test]
fn float_roundtrip_negative() {
    let v = Value::float(-1.5);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some((-1.5f64).to_bits()));
}

#[test]
fn float_roundtrip_infinity() {
    let v = Value::float(f64::INFINITY);
    assert_eq!(
        v.as_float().map(|f| f.to_bits()),
        Some(f64::INFINITY.to_bits())
    );
}

#[test]
fn float_roundtrip_neg_infinity() {
    let v = Value::float(f64::NEG_INFINITY);
    assert_eq!(
        v.as_float().map(|f| f.to_bits()),
        Some(f64::NEG_INFINITY.to_bits())
    );
}

#[test]
fn float_roundtrip_nan() {
    let v = Value::float(f64::NAN);
    assert!(v.as_float().unwrap().is_nan());
}

#[test]
fn float_roundtrip_min() {
    let v = Value::float(f64::MIN);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some(f64::MIN.to_bits()));
}

#[test]
fn float_roundtrip_max() {
    let v = Value::float(f64::MAX);
    assert_eq!(v.as_float().map(|f| f.to_bits()), Some(f64::MAX.to_bits()));
}

#[test]
fn float_roundtrip_min_positive() {
    let v = Value::float(f64::MIN_POSITIVE);
    assert_eq!(
        v.as_float().map(|f| f.to_bits()),
        Some(f64::MIN_POSITIVE.to_bits())
    );
}

#[test]
fn float_roundtrip_epsilon() {
    let v = Value::float(f64::EPSILON);
    assert_eq!(
        v.as_float().map(|f| f.to_bits()),
        Some(f64::EPSILON.to_bits())
    );
}

#[test]
fn float_is_float_type() {
    let v = Value::float(1.5);
    assert!(v.is_float());
    assert!(!v.is_int());
    assert!(!v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_symbol());
    assert!(!v.is_heap());
}

// =========================================================================
// Symbol roundtrip
// =========================================================================

#[test]
fn symbol_roundtrip_zero() {
    let v = Value::symbol(0);
    assert_eq!(v.as_symbol(), Some(0));
}

#[test]
fn symbol_roundtrip_one() {
    let v = Value::symbol(1);
    assert_eq!(v.as_symbol(), Some(1));
}

#[test]
fn symbol_roundtrip_large() {
    let v = Value::symbol(99999);
    assert_eq!(v.as_symbol(), Some(99999));
}

#[test]
fn symbol_is_symbol_type() {
    let v = Value::symbol(42);
    assert!(v.is_symbol());
    assert!(!v.is_int());
    assert!(!v.is_float());
    assert!(!v.is_nil());
    assert!(!v.is_bool());
    assert!(!v.is_keyword());
    assert!(!v.is_heap());
}

// =========================================================================
// Boolean roundtrip
// =========================================================================

#[test]
fn bool_roundtrip_true() {
    let v = Value::bool(true);
    assert_eq!(v.as_bool(), Some(true));
}

#[test]
fn bool_roundtrip_false() {
    let v = Value::bool(false);
    assert_eq!(v.as_bool(), Some(false));
}

// =========================================================================
// Type discrimination: exactly one type predicate is true
// =========================================================================

#[test]
fn exactly_one_type_for_int_min() {
    let v = Value::int(i64::MIN);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_int_max() {
    let v = Value::int(i64::MAX);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_int_zero() {
    let v = Value::int(0);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_float_normal() {
    let v = Value::float(1.5);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_float_zero() {
    let v = Value::float(0.0);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_symbol_zero() {
    let v = Value::symbol(0);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

#[test]
fn exactly_one_type_for_symbol_large() {
    let v = Value::symbol(99999);
    let count = v.is_nil() as u8
        + v.is_empty_list() as u8
        + v.is_bool() as u8
        + v.is_int() as u8
        + v.is_float() as u8
        + v.is_symbol() as u8
        + v.is_keyword() as u8
        + v.is_heap() as u8;
    assert_eq!(count, 1);
}

// =========================================================================
// Truthiness
// =========================================================================

#[test]
fn int_is_truthy_positive() {
    assert!(Value::int(1).is_truthy());
}

#[test]
fn int_is_truthy_negative() {
    assert!(Value::int(-1).is_truthy());
}

#[test]
fn int_is_truthy_zero() {
    assert!(Value::int(0).is_truthy());
}

#[test]
fn float_is_truthy_positive() {
    assert!(Value::float(1.5).is_truthy());
}

#[test]
fn float_is_truthy_negative() {
    assert!(Value::float(-1.5).is_truthy());
}

#[test]
fn float_is_truthy_zero() {
    assert!(Value::float(0.0).is_truthy());
}

#[test]
fn symbol_is_truthy() {
    assert!(Value::symbol(42).is_truthy());
}

#[test]
fn string_is_truthy() {
    assert!(Value::string("hello").is_truthy());
}

#[test]
fn string_empty_is_truthy() {
    assert!(Value::string("").is_truthy());
}

// =========================================================================
// Equality: reflexivity
// =========================================================================

#[test]
fn int_eq_reflexive_min() {
    let v = Value::int(i64::MIN);
    assert_eq!(v, v);
}

#[test]
fn int_eq_reflexive_max() {
    let v = Value::int(i64::MAX);
    assert_eq!(v, v);
}

#[test]
fn int_eq_reflexive_zero() {
    let v = Value::int(0);
    assert_eq!(v, v);
}

#[test]
fn float_eq_reflexive_positive() {
    let v = Value::float(1.5);
    assert_eq!(v, v);
}

#[test]
fn float_eq_reflexive_negative() {
    let v = Value::float(-1.5);
    assert_eq!(v, v);
}

#[test]
fn float_eq_reflexive_zero() {
    let v = Value::float(0.0);
    assert_eq!(v, v);
}

#[test]
fn symbol_eq_reflexive() {
    let v = Value::symbol(42);
    assert_eq!(v, v);
}

// =========================================================================
// Equality: same value, same result
// =========================================================================

#[test]
fn int_eq_same_value_zero() {
    assert_eq!(Value::int(0), Value::int(0));
}

#[test]
fn int_eq_same_value_positive() {
    assert_eq!(Value::int(42), Value::int(42));
}

#[test]
fn int_eq_same_value_negative() {
    assert_eq!(Value::int(-42), Value::int(-42));
}

#[test]
fn int_neq_different_value() {
    assert_ne!(Value::int(0), Value::int(1));
}

#[test]
fn int_neq_different_value_negative() {
    assert_ne!(Value::int(-1), Value::int(1));
}

#[test]
fn int_neq_different_value_large() {
    assert_ne!(Value::int(i64::MIN), Value::int(i64::MAX));
}

#[test]
fn bool_eq_same_value_true() {
    assert_eq!(Value::bool(true), Value::bool(true));
}

#[test]
fn bool_eq_same_value_false() {
    assert_eq!(Value::bool(false), Value::bool(false));
}

// =========================================================================
// Cross-type inequality
// =========================================================================

#[test]
fn int_not_eq_float_zero() {
    // Integers and floats are distinct types; same numeric value means different Values.
    let int_val = Value::int(0);
    let float_val = Value::float(0.0);
    assert!(int_val.is_int() && !int_val.is_float());
    assert!(float_val.is_float() && !float_val.is_int());
    assert_ne!(int_val, float_val);
}

#[test]
fn int_not_eq_float_positive() {
    let int_val = Value::int(1);
    let float_val = Value::float(1.0);
    assert!(int_val.is_int() && !int_val.is_float());
    assert!(float_val.is_float() && !float_val.is_int());
    assert_ne!(int_val, float_val);
}

#[test]
fn int_not_eq_float_negative() {
    let int_val = Value::int(-1);
    let float_val = Value::float(-1.0);
    assert!(int_val.is_int() && !int_val.is_float());
    assert!(float_val.is_float() && !float_val.is_int());
    assert_ne!(int_val, float_val);
}

// =========================================================================
// Cons roundtrip
// =========================================================================

#[test]
fn cons_roundtrip_simple() {
    let car = Value::int(1);
    let cdr = Value::int(2);
    let cons = Value::cons(car, cdr);
    assert!(cons.is_cons());
    assert!(cons.is_heap());
    let c = cons.as_cons().unwrap();
    assert_eq!(c.first, car);
    assert_eq!(c.rest, cdr);
}

#[test]
fn cons_roundtrip_min_max() {
    let car = Value::int(i64::MIN);
    let cdr = Value::int(i64::MAX);
    let cons = Value::cons(car, cdr);
    assert!(cons.is_cons());
    assert!(cons.is_heap());
    let c = cons.as_cons().unwrap();
    assert_eq!(c.first, car);
    assert_eq!(c.rest, cdr);
}

#[test]
fn cons_roundtrip_zero() {
    let car = Value::int(0);
    let cdr = Value::int(0);
    let cons = Value::cons(car, cdr);
    assert!(cons.is_cons());
    assert!(cons.is_heap());
    let c = cons.as_cons().unwrap();
    assert_eq!(c.first, car);
    assert_eq!(c.rest, cdr);
}

// =========================================================================
// String roundtrip
// =========================================================================

#[test]
fn string_roundtrip_empty() {
    let v = Value::string("");
    assert!(v.is_string());
    assert_eq!(v.with_string(|s| s.to_string()), Some("".to_string()));
}

#[test]
fn string_roundtrip_simple() {
    let v = Value::string("hello");
    assert!(v.is_string());
    assert_eq!(v.with_string(|s| s.to_string()), Some("hello".to_string()));
}

#[test]
fn string_roundtrip_with_spaces() {
    let v = Value::string("hello world");
    assert!(v.is_string());
    assert_eq!(
        v.with_string(|s| s.to_string()),
        Some("hello world".to_string())
    );
}

// =========================================================================
// List construction roundtrip
// =========================================================================

#[test]
fn list_roundtrip_empty() {
    let list_val = elle::list(vec![]);
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 0);
}

#[test]
fn list_roundtrip_single() {
    let values = vec![Value::int(42)];
    let list_val = elle::list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0], Value::int(42));
}

#[test]
fn list_roundtrip_multiple() {
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let list_val = elle::list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back[0], Value::int(1));
    assert_eq!(back[1], Value::int(2));
    assert_eq!(back[2], Value::int(3));
}

#[test]
fn list_roundtrip_negative() {
    let values = vec![Value::int(-5), Value::int(0), Value::int(7)];
    let list_val = elle::list(values.clone());
    let back = list_val.list_to_vec().unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back[0], Value::int(-5));
    assert_eq!(back[1], Value::int(0));
    assert_eq!(back[2], Value::int(7));
}

// =========================================================================
// Array roundtrip
// =========================================================================

#[test]
fn array_roundtrip_empty() {
    let arr = Value::array_mut(vec![]);
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 0);
}

#[test]
fn array_roundtrip_single() {
    let values = vec![Value::int(42)];
    let arr = Value::array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 1);
    assert_eq!(borrowed[0], Value::int(42));
}

#[test]
fn array_roundtrip_multiple() {
    let values = vec![Value::int(1), Value::int(2), Value::int(3)];
    let arr = Value::array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0], Value::int(1));
    assert_eq!(borrowed[1], Value::int(2));
    assert_eq!(borrowed[2], Value::int(3));
}

#[test]
fn array_roundtrip_negative() {
    let values = vec![Value::int(-5), Value::int(0), Value::int(7)];
    let arr = Value::array_mut(values.clone());
    assert!(arr.is_array_mut());
    let borrowed = arr.as_array_mut().unwrap().borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0], Value::int(-5));
    assert_eq!(borrowed[1], Value::int(0));
    assert_eq!(borrowed[2], Value::int(7));
}

// =========================================================================
// Constants (not inside proptest! because they don't need generation)
// =========================================================================

#[test]
fn nil_is_falsy() {
    assert!(!Value::NIL.is_truthy());
}

#[test]
fn false_is_falsy() {
    assert!(!Value::FALSE.is_truthy());
}

#[test]
fn empty_list_is_truthy() {
    assert!(Value::EMPTY_LIST.is_truthy());
}

#[test]
fn nil_not_equal_empty_list() {
    assert_ne!(Value::NIL, Value::EMPTY_LIST);
}

#[test]
fn nil_not_equal_false() {
    assert_ne!(Value::NIL, Value::FALSE);
}
