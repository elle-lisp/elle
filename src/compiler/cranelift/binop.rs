// Binary operation compilation for Cranelift
//
// Handles compilation of Elle's binary operations (+, -, *, /)
// to Cranelift IR values. These operations work at the compiler level
// and don't require runtime symbol table lookups.

use crate::value::Value;

/// Computes a binary arithmetic operation on compile-time constants
pub struct BinOpCompiler;

impl BinOpCompiler {
    /// Compute addition of two primitive values
    /// Compute addition of two primitive values
    pub fn add(left: &Value, right: &Value) -> Result<Value, String> {
        match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => Ok(Value::int(a + b)),
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => Ok(Value::float(a + b)),
                _ => {
                    // Handle mixed int+float by coercing int to float
                    match (left.as_int(), right.as_float()) {
                        (Some(a), Some(b)) => Ok(Value::float(a as f64 + b)),
                        _ => match (left.as_float(), right.as_int()) {
                            (Some(a), Some(b)) => Ok(Value::float(a + b as f64)),
                            _ => Err(format!("Cannot add {:?} and {:?}", left, right)),
                        },
                    }
                }
            },
        }
    }

    /// Compute subtraction of two primitive values
    pub fn sub(left: &Value, right: &Value) -> Result<Value, String> {
        match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => Ok(Value::int(a - b)),
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => Ok(Value::float(a - b)),
                _ => Err(format!("Cannot subtract {:?} and {:?}", left, right)),
            },
        }
    }

    /// Compute multiplication of two primitive values
    pub fn mul(left: &Value, right: &Value) -> Result<Value, String> {
        match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => Ok(Value::int(a * b)),
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => Ok(Value::float(a * b)),
                _ => Err(format!("Cannot multiply {:?} and {:?}", left, right)),
            },
        }
    }

    /// Compute division of two primitive values
    pub fn div(left: &Value, right: &Value) -> Result<Value, String> {
        match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => {
                if b == 0 {
                    return Err("Division by zero".to_string());
                }
                Ok(Value::int(a / b))
            }
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => Ok(Value::float(a / b)),
                _ => Err(format!("Cannot divide {:?} and {:?}", left, right)),
            },
        }
    }

    /// Compare two values with less-than
    pub fn lt(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a < b,
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => a < b,
                _ => return Err(format!("Cannot compare {:?} < {:?}", left, right)),
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    /// Compare two values with greater-than
    pub fn gt(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a > b,
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => a > b,
                _ => return Err(format!("Cannot compare {:?} > {:?}", left, right)),
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    /// Compare two values with equality
    pub fn eq(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a == b,
            _ => match (left.as_bool(), right.as_bool()) {
                (Some(a), Some(b)) => a == b,
                _ => match (left.as_float(), right.as_float()) {
                    (Some(a), Some(b)) => a == b,
                    _ => left.is_nil() && right.is_nil(),
                },
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    /// Compare two values with less-than-or-equal
    pub fn lte(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a <= b,
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => a <= b,
                _ => return Err(format!("Cannot compare {:?} <= {:?}", left, right)),
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    /// Compare two values with greater-than-or-equal
    pub fn gte(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a >= b,
            _ => match (left.as_float(), right.as_float()) {
                (Some(a), Some(b)) => a >= b,
                _ => return Err(format!("Cannot compare {:?} >= {:?}", left, right)),
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    /// Compare two values with not-equal
    pub fn neq(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left.as_int(), right.as_int()) {
            (Some(a), Some(b)) => a != b,
            _ => match (left.as_bool(), right.as_bool()) {
                (Some(a), Some(b)) => a != b,
                _ => match (left.as_float(), right.as_float()) {
                    (Some(a), Some(b)) => a != b,
                    _ => !(left.is_nil() && right.is_nil()),
                },
            },
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_ints() {
        let result = BinOpCompiler::add(&Value::int(1), &Value::int(2));
        assert_eq!(result, Ok(Value::int(3)));
    }

    #[test]
    fn test_add_floats() {
        let result = BinOpCompiler::add(&Value::float(1.5), &Value::float(2.5));
        assert_eq!(result, Ok(Value::float(4.0)));
    }

    #[test]
    fn test_add_mixed() {
        let result = BinOpCompiler::add(&Value::int(1), &Value::float(2.5));
        assert_eq!(result, Ok(Value::float(3.5)));
    }

    #[test]
    fn test_sub_ints() {
        let result = BinOpCompiler::sub(&Value::int(5), &Value::int(3));
        assert_eq!(result, Ok(Value::int(2)));
    }

    #[test]
    fn test_mul_ints() {
        let result = BinOpCompiler::mul(&Value::int(3), &Value::int(4));
        assert_eq!(result, Ok(Value::int(12)));
    }

    #[test]
    fn test_div_ints() {
        let result = BinOpCompiler::div(&Value::int(10), &Value::int(2));
        assert_eq!(result, Ok(Value::int(5)));
    }

    #[test]
    fn test_div_by_zero() {
        let result = BinOpCompiler::div(&Value::int(1), &Value::int(0));
        assert!(result.is_err());
    }

    #[test]
    fn test_lt_true() {
        let result = BinOpCompiler::lt(&Value::int(1), &Value::int(2));
        assert_eq!(result, Ok(Value::TRUE));
    }

    #[test]
    fn test_lt_false() {
        let result = BinOpCompiler::lt(&Value::int(2), &Value::int(1));
        assert_eq!(result, Ok(Value::FALSE));
    }

    #[test]
    fn test_eq_ints() {
        let result = BinOpCompiler::eq(&Value::int(5), &Value::int(5));
        assert_eq!(result, Ok(Value::TRUE));
    }

    #[test]
    fn test_eq_nils() {
        let result = BinOpCompiler::eq(&Value::NIL, &Value::NIL);
        assert_eq!(result, Ok(Value::TRUE));
    }

    #[test]
    fn test_neq_ints() {
        let result = BinOpCompiler::neq(&Value::int(1), &Value::int(2));
        assert_eq!(result, Ok(Value::TRUE));
    }
}
