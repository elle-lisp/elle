//! Unified arithmetic operations for both VM and primitives
//!
//! This module provides a single source of truth for arithmetic operations
//! (add, subtract, multiply, divide, etc.) to avoid duplication between
//! the VM's binary stack operations and the primitives' variadic functions.
//!
//! All functions return `Result<Value, Value>` where `Err` is an
//! already-composed Elle error struct (e.g. `{:error :overflow ...}`).

use crate::value::{error_val, Value};

/// Add two numeric values, promoting to float when either operand is float.
pub(crate) fn add_values(a: &Value, b: &Value) -> Result<Value, Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_add(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "+: integer overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x + y)),
        _ => Err(error_val(
            "type-error",
            format!(
                "+: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Subtract two numeric values, promoting to float when either operand is float.
pub(crate) fn sub_values(a: &Value, b: &Value) -> Result<Value, Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_sub(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "-: integer overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x - y)),
        _ => Err(error_val(
            "type-error",
            format!(
                "-: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Multiply two numeric values, promoting to float when either operand is float.
pub(crate) fn mul_values(a: &Value, b: &Value) -> Result<Value, Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return match x.checked_mul(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "*: integer overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x * y)),
        _ => Err(error_val(
            "type-error",
            format!(
                "*: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Divide two numeric values. Integer division truncates; mixed/float
/// division follows IEEE 754 (including Inf on divide-by-zero).
pub(crate) fn div_values(a: &Value, b: &Value) -> Result<Value, Value> {
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        if y == 0 {
            return Err(error_val("division-by-zero", "/: division by zero"));
        }
        return match x.checked_div(y) {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "/: integer overflow")),
        };
    }
    match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) => Ok(Value::float(x / y)),
        _ => Err(error_val(
            "type-error",
            format!(
                "/: expected number, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Negate a numeric value
pub(crate) fn negate_value(a: &Value) -> Result<Value, Value> {
    if let Some(n) = a.as_int() {
        return match n.checked_neg() {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "negate: integer overflow")),
        };
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(-f)),
        None => Err(error_val(
            "type-error",
            format!("negate: expected number, got {}", a.type_name()),
        )),
    }
}

/// Reciprocal of a numeric value (1/x). Integer zero errors;
/// float zero returns Inf per IEEE 754.
pub(crate) fn reciprocal_value(a: &Value) -> Result<Value, Value> {
    if let Some(n) = a.as_int() {
        if n == 0 {
            return Err(error_val(
                "division-by-zero",
                "reciprocal: division by zero",
            ));
        }
        return Ok(Value::float(1.0 / n as f64));
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(1.0 / f)),
        None => Err(error_val(
            "type-error",
            format!("reciprocal: expected number, got {}", a.type_name()),
        )),
    }
}

/// Modulo operation (Euclidean modulo - result has same sign as divisor)
pub(crate) fn mod_values(a: &Value, b: &Value) -> Result<Value, Value> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err(error_val("division-by-zero", "mod: division by zero"));
            }
            Ok(Value::int(x.rem_euclid(y)))
        }
        _ => Err(error_val(
            "type-error",
            format!(
                "mod: expected integer, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Remainder operation (truncated division - result has same sign as dividend)
pub(crate) fn remainder_values(a: &Value, b: &Value) -> Result<Value, Value> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err(error_val("division-by-zero", "rem: division by zero"));
            }
            Ok(Value::int(x % y))
        }
        _ => Err(error_val(
            "type-error",
            format!(
                "rem: expected integer, got {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

/// Absolute value of a numeric value
pub(crate) fn abs_value(a: &Value) -> Result<Value, Value> {
    if let Some(n) = a.as_int() {
        return match n.checked_abs() {
            Some(r) => Ok(Value::int(r)),
            None => Err(error_val("overflow", "abs: integer overflow")),
        };
    }
    match a.as_float() {
        Some(f) => Ok(Value::float(f.abs())),
        None => Err(error_val(
            "type-error",
            format!("abs: expected number, got {}", a.type_name()),
        )),
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
