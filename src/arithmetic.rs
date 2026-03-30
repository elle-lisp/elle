//! Unified arithmetic operations for both VM and primitives
//!
//! This module provides a single source of truth for arithmetic operations
//! (add, subtract, multiply, divide, etc.) to avoid duplication between
//! the VM's binary stack operations and the primitives' variadic functions.

use crate::error::{LError, LResult};
use crate::value::Value;

/// Add two numeric values, promoting to float when either operand is float.
pub(crate) fn add_values(a: &Value, b: &Value) -> LResult<Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_add(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer addition overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x + y)),
        _ => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Subtract two numeric values, promoting to float when either operand is float.
pub(crate) fn sub_values(a: &Value, b: &Value) -> LResult<Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_sub(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer subtraction overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x - y)),
        _ => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Multiply two numeric values, promoting to float when either operand is float.
pub(crate) fn mul_values(a: &Value, b: &Value) -> LResult<Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_mul(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer multiplication overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x * y)),
        _ => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Divide two numeric values. Integer division truncates; mixed/float
/// division follows IEEE 754 (including Inf on divide-by-zero).
pub(crate) fn div_values(a: &Value, b: &Value) -> LResult<Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        if y == 0 {
            return Err(LError::division_by_zero());
        }
        return match x.checked_div(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer division overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x / y)),
        _ => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Negate a numeric value
pub(crate) fn negate_value(a: &Value) -> LResult<Value> {
    if let Some(n) = a.as_int() {
        return match n.checked_neg() {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer negation overflow")),
        };
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(-f)),
        None => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Reciprocal of a numeric value (1/x). Integer zero errors;
/// float zero returns Inf per IEEE 754.
pub(crate) fn reciprocal_value(a: &Value) -> LResult<Value> {
    if let Some(n) = a.as_int() {
        if n == 0 {
            return Err(LError::division_by_zero());
        }
        return Ok(Value::float(1.0 / n as f64));
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(1.0 / f)),
        None => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Modulo operation (Euclidean modulo - result has same sign as divisor)
pub(crate) fn mod_values(a: &Value, b: &Value) -> LResult<Value> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err(LError::division_by_zero());
            }
            Ok(Value::int(x.rem_euclid(y)))
        }
        _ => Err(LError::type_mismatch("integer", "non-integer value")),
    }
}

/// Remainder operation (truncated division - result has same sign as dividend)
pub(crate) fn remainder_values(a: &Value, b: &Value) -> LResult<Value> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err(LError::division_by_zero());
            }
            Ok(Value::int(x % y))
        }
        _ => Err(LError::type_mismatch("integer", "non-integer value")),
    }
}

/// Absolute value of a numeric value
pub(crate) fn abs_value(a: &Value) -> LResult<Value> {
    if let Some(n) = a.as_int() {
        return match n.checked_abs() {
            Some(r) => Ok(Value::int(r)),
            None => Err(LError::numeric_overflow("integer abs overflow")),
        };
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(f.abs())),
        None => Err(LError::type_mismatch("number", "non-numeric value")),
    }
}

/// Get minimum of two numeric values
pub(crate) fn min_values(a: &Value, b: &Value) -> Value {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Value::int(x.min(y)),
        _ => {
            let af = a.as_number().unwrap();
            let bf = b.as_number().unwrap();
            if af <= bf {
                *a
            } else {
                *b
            }
        }
    }
}

/// Get maximum of two numeric values
pub(crate) fn max_values(a: &Value, b: &Value) -> Value {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Value::int(x.max(y)),
        _ => {
            let af = a.as_number().unwrap();
            let bf = b.as_number().unwrap();
            if af >= bf {
                *a
            } else {
                *b
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn test_div_by_zero_int() {
        let a = Value::int(5);
        let b = Value::int(0);
        assert!(div_values(&a, &b).is_err());
    }

    #[test]
    fn test_mod_values() {
        let a = Value::int(17);
        let b = Value::int(5);
        assert_eq!(mod_values(&a, &b).unwrap(), Value::int(2));
    }

    #[test]
    fn test_abs_value() {
        let a = Value::int(-5);
        assert_eq!(abs_value(&a).unwrap(), Value::int(5));
    }

    #[test]
    fn test_negate_value() {
        let a = Value::int(5);
        assert_eq!(negate_value(&a).unwrap(), Value::int(-5));
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
    fn test_div_by_zero_float() {
        let a = Value::float(10.0);
        let b = Value::int(0);
        assert!(div_values(&a, &b).is_err());
    }
}
