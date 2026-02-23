//! Unified arithmetic operations for both VM and primitives
//!
//! This module provides a single source of truth for arithmetic operations
//! (add, subtract, multiply, divide, etc.) to avoid duplication between
//! the VM's binary stack operations and the primitives' variadic functions.

use crate::value::Value;

/// Add two numeric values, automatically promoting Int to Float when needed
/// Add two numeric values, automatically promoting Int to Float when needed
pub fn add_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Ok(Value::int(x + y)),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Ok(Value::float(x + y)),
            _ => {
                // Handle mixed int+float by coercing int to float
                match (a.as_int(), b.as_float()) {
                    (Some(x), Some(y)) => Ok(Value::float(x as f64 + y)),
                    _ => match (a.as_float(), b.as_int()) {
                        (Some(x), Some(y)) => Ok(Value::float(x + y as f64)),
                        _ => Err("Type error: + requires numbers".to_string()),
                    },
                }
            }
        },
    }
}

/// Subtract two numeric values, automatically promoting Int to Float when needed
pub fn sub_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Ok(Value::int(x - y)),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Ok(Value::float(x - y)),
            _ => {
                // Handle mixed int-float by coercing int to float
                match (a.as_int(), b.as_float()) {
                    (Some(x), Some(y)) => Ok(Value::float(x as f64 - y)),
                    _ => match (a.as_float(), b.as_int()) {
                        (Some(x), Some(y)) => Ok(Value::float(x - y as f64)),
                        _ => Err("Type error: - requires numbers".to_string()),
                    },
                }
            }
        },
    }
}

/// Multiply two numeric values, automatically promoting Int to Float when needed
pub fn mul_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Ok(Value::int(x * y)),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Ok(Value::float(x * y)),
            _ => {
                // Handle mixed int*float by coercing int to float
                match (a.as_int(), b.as_float()) {
                    (Some(x), Some(y)) => Ok(Value::float(x as f64 * y)),
                    _ => match (a.as_float(), b.as_int()) {
                        (Some(x), Some(y)) => Ok(Value::float(x * y as f64)),
                        _ => Err("Type error: * requires numbers".to_string()),
                    },
                }
            }
        },
    }
}

/// Divide two numeric values, automatically promoting Int to Float when needed
pub fn div_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err("Division by zero".to_string());
            }
            Ok(Value::int(x / y))
        }
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Ok(Value::float(x / y)),
            _ => {
                // Handle mixed int/float by coercing int to float
                match (a.as_int(), b.as_float()) {
                    (Some(x), Some(y)) => Ok(Value::float(x as f64 / y)),
                    _ => match (a.as_float(), b.as_int()) {
                        (Some(x), Some(y)) => {
                            if y == 0 {
                                return Err("Division by zero".to_string());
                            }
                            Ok(Value::float(x / y as f64))
                        }
                        _ => Err("Type error: / requires numbers".to_string()),
                    },
                }
            }
        },
    }
}

/// Negate a numeric value
pub fn negate_value(a: &Value) -> Result<Value, String> {
    match a.as_int() {
        Some(n) => Ok(Value::int(-n)),
        None => match a.as_float() {
            Some(f) => Ok(Value::float(-f)),
            None => Err("Type error: negate requires a number".to_string()),
        },
    }
}

/// Reciprocal of a numeric value (1/x)
pub fn reciprocal_value(a: &Value) -> Result<Value, String> {
    match a.as_int() {
        Some(n) => {
            if n == 0 {
                Err("Division by zero".to_string())
            } else {
                Ok(Value::float(1.0 / n as f64))
            }
        }
        None => match a.as_float() {
            Some(f) => Ok(Value::float(1.0 / f)),
            None => Err("Type error: reciprocal requires a number".to_string()),
        },
    }
}

/// Modulo operation (Euclidean modulo - result has same sign as divisor)
pub fn mod_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err("Modulo by zero".to_string());
            }
            Ok(Value::int(x.rem_euclid(y)))
        }
        _ => Err("Type error: mod requires integers".to_string()),
    }
}

/// Remainder operation (truncated division - result has same sign as dividend)
pub fn remainder_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => {
            if y == 0 {
                return Err("Remainder by zero".to_string());
            }
            Ok(Value::int(x % y))
        }
        _ => Err("Type error: remainder requires integers".to_string()),
    }
}

/// Absolute value of a numeric value
pub fn abs_value(a: &Value) -> Result<Value, String> {
    match a.as_int() {
        Some(n) => Ok(Value::int(n.abs())),
        None => match a.as_float() {
            Some(f) => Ok(Value::float(f.abs())),
            None => Err("Type error: abs requires a number".to_string()),
        },
    }
}

/// Get minimum of two numeric values
pub fn min_values(a: &Value, b: &Value) -> Value {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Value::int(x.min(y)),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Value::float(x.min(y)),
            _ => *a,
        },
    }
}

/// Get maximum of two numeric values
pub fn max_values(a: &Value, b: &Value) -> Value {
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Value::int(x.max(y)),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => Value::float(x.max(y)),
            _ => *a,
        },
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
