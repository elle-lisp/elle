// Property-based tests for conversion primitives
use crate::common::eval_source;
use elle::Value;
use proptest::prelude::*;

// === integer conversion roundtrip ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn integer_from_int_is_identity(n in -10000i64..=10000) {
        let source = format!("(integer {})", n);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn integer_from_string_roundtrip(n in -10000i64..=10000) {
        let source = format!("(integer \"{}\")", n);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}

// === float conversion roundtrip ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn float_from_int_preserves_value(n in -10000i64..=10000) {
        let source = format!("(float {})", n);
        let result = eval_source(&source).unwrap();
        let f = result.as_float().expect("should be float");
        prop_assert!((f - n as f64).abs() < f64::EPSILON);
    }
}

// === number->string roundtrip ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn number_to_string_int_roundtrip(n in -10000i64..=10000) {
        let source = format!("(string->integer (number->string {}))", n);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}

// === string conversion ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn string_from_int_matches_format(n in -10000i64..=10000) {
        let source = format!("(string {})", n);
        let result = eval_source(&source).unwrap();
        let s = result.as_string().expect("should be string");
        prop_assert_eq!(s, &n.to_string());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn string_from_bool_is_correct(b in prop::bool::ANY) {
        let source = if b { "(string true)" } else { "(string false)" };
        let result = eval_source(source).unwrap();
        let s = result.as_string().expect("should be string");
        let expected = if b { "true" } else { "false" };
        prop_assert_eq!(s, expected);
    }
}

// === integer truncation from float ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn integer_from_float_truncates(n in -1000i64..=1000) {
        // integer(float(n)) should equal n (no fractional part)
        let source = format!("(integer (float {}))", n);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}

// === keyword->string strips colon ===

#[test]
fn keyword_to_string_strips_colon() {
    assert_eq!(
        eval_source("(keyword->string :hello)").unwrap(),
        Value::string("hello")
    );
}

#[test]
fn keyword_to_string_single_char() {
    assert_eq!(
        eval_source("(keyword->string :x)").unwrap(),
        Value::string("x")
    );
}

// === any->string handles all types ===

#[test]
fn any_to_string_nil() {
    assert_eq!(
        eval_source("(any->string nil)").unwrap(),
        Value::string("nil")
    );
}

#[test]
fn any_to_string_list() {
    let result = eval_source("(any->string (list 1 2))").unwrap();
    let s = result.as_string().expect("should be string");
    assert!(s.contains("1"));
    assert!(s.contains("2"));
}
