// Property tests for NaN-boxed Value representation.
//
// These tests verify the fundamental invariants of the Value type:
// roundtrip fidelity, type discrimination, truthiness, and equality.

use elle::value::repr::{INT_MAX, INT_MIN};
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    // =========================================================================
    // Integer roundtrip
    // =========================================================================

    #[test]
    fn int_roundtrip(n in INT_MIN..=INT_MAX) {
        let v = Value::int(n);
        prop_assert_eq!(v.as_int(), Some(n), "int roundtrip failed for {}", n);
    }

    #[test]
    fn int_is_int(n in INT_MIN..=INT_MAX) {
        let v = Value::int(n);
        prop_assert!(v.is_int());
        prop_assert!(!v.is_float());
        prop_assert!(!v.is_nil());
        prop_assert!(!v.is_bool());
        prop_assert!(!v.is_symbol());
        prop_assert!(!v.is_keyword());
        prop_assert!(!v.is_heap());
        prop_assert!(!v.is_empty_list());
    }

    // =========================================================================
    // Float roundtrip
    // =========================================================================

    #[test]
    fn float_roundtrip_normal(f in prop::num::f64::NORMAL) {
        let v = Value::float(f);
        let extracted = v.as_float();
        prop_assert!(extracted.is_some(), "as_float() returned None for {}", f);
        prop_assert_eq!(extracted.unwrap().to_bits(), f.to_bits(),
            "float roundtrip failed for {} (bits: {:016x})", f, f.to_bits());
    }

    #[test]
    fn float_roundtrip_special(f in prop_oneof![
        Just(0.0f64),
        Just(-0.0f64),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(f64::NAN),
        Just(f64::MIN),
        Just(f64::MAX),
        Just(f64::MIN_POSITIVE),
        Just(f64::EPSILON),
    ]) {
        let v = Value::float(f);
        let extracted = v.as_float();
        prop_assert!(extracted.is_some(), "as_float() returned None for {:?}", f);
        if f.is_nan() {
            prop_assert!(extracted.unwrap().is_nan(), "NaN roundtrip failed");
        } else {
            prop_assert_eq!(extracted.unwrap().to_bits(), f.to_bits(),
                "float roundtrip failed for {:?}", f);
        }
    }

    #[test]
    fn float_is_float(f in prop::num::f64::NORMAL) {
        let v = Value::float(f);
        prop_assert!(v.is_float());
        prop_assert!(!v.is_int());
        prop_assert!(!v.is_nil());
        prop_assert!(!v.is_bool());
        prop_assert!(!v.is_symbol());
        prop_assert!(!v.is_heap());
    }

    // =========================================================================
    // Symbol roundtrip
    // =========================================================================

    #[test]
    fn symbol_roundtrip(id in 0u32..100000) {
        let v = Value::symbol(id);
        prop_assert_eq!(v.as_symbol(), Some(id));
    }

    #[test]
    fn symbol_is_symbol(id in 0u32..100000) {
        let v = Value::symbol(id);
        prop_assert!(v.is_symbol());
        prop_assert!(!v.is_int());
        prop_assert!(!v.is_float());
        prop_assert!(!v.is_nil());
        prop_assert!(!v.is_bool());
        prop_assert!(!v.is_keyword());
        prop_assert!(!v.is_heap());
    }

    // =========================================================================
    // Boolean roundtrip
    // =========================================================================

    #[test]
    fn bool_roundtrip(b in prop::bool::ANY) {
        let v = Value::bool(b);
        prop_assert_eq!(v.as_bool(), Some(b));
    }

    // =========================================================================
    // Type discrimination: exactly one type predicate is true
    // =========================================================================

    #[test]
    fn exactly_one_type_for_int(n in INT_MIN..=INT_MAX) {
        let v = Value::int(n);
        let count = v.is_nil() as u8
            + v.is_empty_list() as u8
            + v.is_bool() as u8
            + v.is_int() as u8
            + v.is_float() as u8
            + v.is_symbol() as u8
            + v.is_keyword() as u8
            + v.is_heap() as u8;
        prop_assert_eq!(count, 1, "Expected exactly 1 type predicate true for int {}, got {}", n, count);
    }

    #[test]
    fn exactly_one_type_for_float(f in prop::num::f64::NORMAL) {
        let v = Value::float(f);
        let count = v.is_nil() as u8
            + v.is_empty_list() as u8
            + v.is_bool() as u8
            + v.is_int() as u8
            + v.is_float() as u8
            + v.is_symbol() as u8
            + v.is_keyword() as u8
            + v.is_heap() as u8;
        prop_assert_eq!(count, 1, "Expected exactly 1 type predicate true for float {}, got {}", f, count);
    }

    #[test]
    fn exactly_one_type_for_symbol(id in 0u32..100000) {
        let v = Value::symbol(id);
        let count = v.is_nil() as u8
            + v.is_empty_list() as u8
            + v.is_bool() as u8
            + v.is_int() as u8
            + v.is_float() as u8
            + v.is_symbol() as u8
            + v.is_keyword() as u8
            + v.is_heap() as u8;
        prop_assert_eq!(count, 1, "Expected exactly 1 type predicate true for symbol {}, got {}", id, count);
    }

    // =========================================================================
    // Truthiness
    // =========================================================================

    #[test]
    fn int_is_truthy(n in INT_MIN..=INT_MAX) {
        prop_assert!(Value::int(n).is_truthy(), "int {} should be truthy", n);
    }

    #[test]
    fn float_is_truthy(f in prop::num::f64::NORMAL) {
        prop_assert!(Value::float(f).is_truthy(), "float {} should be truthy", f);
    }

    #[test]
    fn symbol_is_truthy(id in 0u32..100000) {
        prop_assert!(Value::symbol(id).is_truthy());
    }

    #[test]
    fn string_is_truthy(s in "[a-zA-Z0-9]{0,20}") {
        prop_assert!(Value::string(s).is_truthy(), "strings are always truthy");
    }

    // =========================================================================
    // Equality: reflexivity
    // =========================================================================

    #[test]
    fn int_eq_reflexive(n in INT_MIN..=INT_MAX) {
        let v = Value::int(n);
        prop_assert_eq!(v, v, "int {} should equal itself", n);
    }

    #[test]
    fn float_eq_reflexive(f in prop::num::f64::NORMAL.prop_filter("NaN breaks reflexivity", |f| !f.is_nan())) {
        let v = Value::float(f);
        prop_assert_eq!(v, v, "float {} should equal itself", f);
    }

    #[test]
    fn symbol_eq_reflexive(id in 0u32..100000) {
        let v = Value::symbol(id);
        prop_assert_eq!(v, v);
    }

    // =========================================================================
    // Equality: same value, same result
    // =========================================================================

    #[test]
    fn int_eq_same_value(n in INT_MIN..=INT_MAX) {
        prop_assert_eq!(Value::int(n), Value::int(n));
    }

    #[test]
    fn int_neq_different_value(a in INT_MIN..=INT_MAX, b in INT_MIN..=INT_MAX) {
        prop_assume!(a != b);
        prop_assert_ne!(Value::int(a), Value::int(b));
    }

    #[test]
    fn bool_eq_same_value(b in prop::bool::ANY) {
        prop_assert_eq!(Value::bool(b), Value::bool(b));
    }

    // =========================================================================
    // Cross-type inequality
    // =========================================================================

    #[test]
    fn int_not_eq_float(n in -1000i64..1000) {
        // Even when the numeric values are "equal", int != float as types
        let int_val = Value::int(n);
        let float_val = Value::float(n as f64);
        // They're different types (different bit representations)
        prop_assert_ne!(int_val.to_bits(), float_val.to_bits(),
            "int and float should have different bit patterns");
    }

    // =========================================================================
    // Cons roundtrip
    // =========================================================================

    #[test]
    fn cons_roundtrip(
        car_n in INT_MIN..=INT_MAX,
        cdr_n in INT_MIN..=INT_MAX,
    ) {
        let car = Value::int(car_n);
        let cdr = Value::int(cdr_n);
        let cons = Value::cons(car, cdr);
        prop_assert!(cons.is_cons());
        prop_assert!(cons.is_heap());
        let c = cons.as_cons().unwrap();
        prop_assert_eq!(c.first, car);
        prop_assert_eq!(c.rest, cdr);
    }

    // =========================================================================
    // String roundtrip
    // =========================================================================

    #[test]
    fn string_roundtrip(s in "[a-zA-Z0-9_ ]{0,50}") {
        let v = Value::string(s.clone());
        prop_assert!(v.is_string());
        prop_assert_eq!(v.as_string(), Some(s.as_str()));
    }

    // =========================================================================
    // List construction roundtrip
    // =========================================================================

    #[test]
    fn list_roundtrip(elems in prop::collection::vec(INT_MIN..=INT_MAX, 0..=10)) {
        let values: Vec<Value> = elems.iter().map(|&n| Value::int(n)).collect();
        let list_val = elle::list(values);
        let back = list_val.list_to_vec().unwrap();
        prop_assert_eq!(back.len(), elems.len());
        for (i, &n) in elems.iter().enumerate() {
            prop_assert_eq!(back[i], Value::int(n), "mismatch at index {}", i);
        }
    }

    // =========================================================================
    // Array roundtrip
    // =========================================================================

    #[test]
    fn array_roundtrip(elems in prop::collection::vec(INT_MIN..=INT_MAX, 0..=10)) {
        let values: Vec<Value> = elems.iter().map(|&n| Value::int(n)).collect();
        let arr = Value::array(values);
        prop_assert!(arr.is_array());
        let borrowed = arr.as_array().unwrap().borrow();
        prop_assert_eq!(borrowed.len(), elems.len());
        for (i, &n) in elems.iter().enumerate() {
            prop_assert_eq!(borrowed[i], Value::int(n), "mismatch at index {}", i);
        }
    }
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
