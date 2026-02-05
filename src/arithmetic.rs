//! Unified arithmetic operations for both VM and primitives
//!
//! This module provides a single source of truth for arithmetic operations
//! (add, subtract, multiply, divide, etc.) to avoid duplication between
//! the VM's binary stack operations and the primitives' variadic functions.

use crate::value::Value;

/// Add two numeric values, automatically promoting Int to Float when needed
pub fn add_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 + y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x + *y as f64)),
        _ => Err("Type error: + requires numbers".to_string()),
    }
}

/// Subtract two numeric values, automatically promoting Int to Float when needed
pub fn sub_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 - y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x - *y as f64)),
        _ => Err("Type error: - requires numbers".to_string()),
    }
}

/// Multiply two numeric values, automatically promoting Int to Float when needed
pub fn mul_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 * y)),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x * *y as f64)),
        _ => Err("Type error: * requires numbers".to_string()),
    }
}

/// Divide two numeric values, automatically promoting Int to Float when needed
pub fn div_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                return Err("Division by zero".to_string());
            }
            Ok(Value::Int(x / y))
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 / y)),
        (Value::Float(x), Value::Int(y)) => {
            if *y == 0 {
                return Err("Division by zero".to_string());
            }
            Ok(Value::Float(x / *y as f64))
        }
        _ => Err("Type error: / requires numbers".to_string()),
    }
}

/// Negate a numeric value
pub fn negate_value(a: &Value) -> Result<Value, String> {
    match a {
        Value::Int(n) => Ok(Value::Int(-n)),
        Value::Float(f) => Ok(Value::Float(-f)),
        _ => Err("Type error: negate requires a number".to_string()),
    }
}

/// Reciprocal of a numeric value (1/x)
pub fn reciprocal_value(a: &Value) -> Result<Value, String> {
    match a {
        Value::Int(n) => {
            if *n == 0 {
                Err("Division by zero".to_string())
            } else {
                Ok(Value::Float(1.0 / *n as f64))
            }
        }
        Value::Float(f) => Ok(Value::Float(1.0 / f)),
        _ => Err("Type error: reciprocal requires a number".to_string()),
    }
}

/// Modulo operation (Euclidean modulo - result has same sign as divisor)
pub fn mod_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                return Err("Modulo by zero".to_string());
            }
            Ok(Value::Int(x.rem_euclid(*y)))
        }
        _ => Err("Type error: mod requires integers".to_string()),
    }
}

/// Remainder operation (truncated division - result has same sign as dividend)
pub fn remainder_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                return Err("Remainder by zero".to_string());
            }
            Ok(Value::Int(x % y))
        }
        _ => Err("Type error: remainder requires integers".to_string()),
    }
}

/// Absolute value of a numeric value
pub fn abs_value(a: &Value) -> Result<Value, String> {
    match a {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        _ => Err("Type error: abs requires a number".to_string()),
    }
}

/// Get minimum of two numeric values
pub fn min_values(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int((*x).min(*y)),
        (Value::Int(x), Value::Float(y)) => {
            let x_float = *x as f64;
            if x_float < *y {
                Value::Int(*x)
            } else {
                Value::Float(*y)
            }
        }
        (Value::Float(x), Value::Int(y)) => {
            let y_float = *y as f64;
            if *x < y_float {
                Value::Float(*x)
            } else {
                Value::Int(*y)
            }
        }
        (Value::Float(x), Value::Float(y)) => Value::Float(x.min(*y)),
        _ => a.clone(),
    }
}

/// Get maximum of two numeric values
pub fn max_values(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int((*x).max(*y)),
        (Value::Int(x), Value::Float(y)) => {
            let x_float = *x as f64;
            if x_float > *y {
                Value::Int(*x)
            } else {
                Value::Float(*y)
            }
        }
        (Value::Float(x), Value::Int(y)) => {
            let y_float = *y as f64;
            if *x > y_float {
                Value::Float(*x)
            } else {
                Value::Int(*y)
            }
        }
        (Value::Float(x), Value::Float(y)) => Value::Float(x.max(*y)),
        _ => a.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_int_int() {
        let a = Value::Int(5);
        let b = Value::Int(3);
        assert_eq!(add_values(&a, &b).unwrap(), Value::Int(8));
    }

    #[test]
    fn test_add_int_float() {
        let a = Value::Int(5);
        let b = Value::Float(3.5);
        let result = add_values(&a, &b).unwrap();
        assert!(matches!(result, Value::Float(f) if (f - 8.5).abs() < 0.001));
    }

    #[test]
    fn test_div_by_zero_int() {
        let a = Value::Int(5);
        let b = Value::Int(0);
        assert!(div_values(&a, &b).is_err());
    }

    #[test]
    fn test_mod_values() {
        let a = Value::Int(17);
        let b = Value::Int(5);
        assert_eq!(mod_values(&a, &b).unwrap(), Value::Int(2));
    }

    #[test]
    fn test_abs_value() {
        let a = Value::Int(-5);
        assert_eq!(abs_value(&a).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_negate_value() {
        let a = Value::Int(5);
        assert_eq!(negate_value(&a).unwrap(), Value::Int(-5));
    }
}
