//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_add_int_int() {
    let a = Value::int(5);
    let b = Value::int(3);
    assert_eq!(add_values(&a, &b).unwrap(), Value::int(8));
}

#[test]
fn test_add_int_float() {
    let a = Value::int(5);
    let b = Value::float(3.5);
    let result = add_values(&a, &b).unwrap();
    assert!(result.as_float().is_some_and(|f| (f - 8.5).abs() < 0.001));
}

#[test]
fn test_sub_int_float() {
    let a = Value::int(10);
    let b = Value::float(3.5);
    let result = sub_values(&a, &b).unwrap();
    assert!(result.as_float().is_some_and(|f| (f - 6.5).abs() < 0.001));
}

#[test]
fn test_sub_float_int() {
    let a = Value::float(10.5);
    let b = Value::int(3);
    let result = sub_values(&a, &b).unwrap();
    assert!(result.as_float().is_some_and(|f| (f - 7.5).abs() < 0.001));
}

#[test]
fn test_div_int_float() {
    let a = Value::int(10);
    let b = Value::float(2.5);
    let result = div_values(&a, &b).unwrap();
    assert!(result.as_float().is_some_and(|f| (f - 4.0).abs() < 0.001));
}

#[test]
fn test_div_float_int() {
    let a = Value::float(10.0);
    let b = Value::int(4);
    let result = div_values(&a, &b).unwrap();
    assert!(result.as_float().is_some_and(|f| (f - 2.5).abs() < 0.001));
}

#[test]
fn test_div_by_zero_float_returns_inf() {
    // Mixed float/int division follows IEEE 754: float / 0 = Inf.
    // Only int / int zero is an error.
    let a = Value::float(10.0);
    let b = Value::int(0);
    let result = div_values(&a, &b).unwrap();
    assert!(result.as_float().unwrap().is_infinite());
}
