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
    pub fn add(left: &Value, right: &Value) -> Result<Value, String> {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            _ => Err(format!("Cannot add {:?} and {:?}", left, right)),
        }
    }

    /// Compute subtraction of two primitive values
    pub fn sub(left: &Value, right: &Value) -> Result<Value, String> {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            _ => Err(format!("Cannot subtract {:?} and {:?}", left, right)),
        }
    }

    /// Compute multiplication of two primitive values
    pub fn mul(left: &Value, right: &Value) -> Result<Value, String> {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
            _ => Err(format!("Cannot multiply {:?} and {:?}", left, right)),
        }
    }

    /// Compute division of two primitive values
    pub fn div(left: &Value, right: &Value) -> Result<Value, String> {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    return Err("Division by zero".to_string());
                }
                Ok(Value::Int(a / b))
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
            _ => Err(format!("Cannot divide {:?} and {:?}", left, right)),
        }
    }

    /// Compare two values with less-than
    pub fn lt(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a < b,
            (Value::Float(a), Value::Float(b)) => a < b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) < *b,
            (Value::Float(a), Value::Int(b)) => *a < (*b as f64),
            _ => return Err(format!("Cannot compare {:?} < {:?}", left, right)),
        };
        Ok(Value::Bool(result))
    }

    /// Compare two values with greater-than
    pub fn gt(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a > b,
            (Value::Float(a), Value::Float(b)) => a > b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) > *b,
            (Value::Float(a), Value::Int(b)) => *a > (*b as f64),
            _ => return Err(format!("Cannot compare {:?} > {:?}", left, right)),
        };
        Ok(Value::Bool(result))
    }

    /// Compare two values with equality
    pub fn eq(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => *a as f64 == *b,
            (Value::Float(a), Value::Int(b)) => *a == *b as f64,
            (Value::Nil, Value::Nil) => true,
            _ => false,
        };
        Ok(Value::Bool(result))
    }

    /// Compare two values with less-than-or-equal
    pub fn lte(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a <= b,
            (Value::Float(a), Value::Float(b)) => a <= b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) <= *b,
            (Value::Float(a), Value::Int(b)) => *a <= (*b as f64),
            _ => return Err(format!("Cannot compare {:?} <= {:?}", left, right)),
        };
        Ok(Value::Bool(result))
    }

    /// Compare two values with greater-than-or-equal
    pub fn gte(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a >= b,
            (Value::Float(a), Value::Float(b)) => a >= b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) >= *b,
            (Value::Float(a), Value::Int(b)) => *a >= (*b as f64),
            _ => return Err(format!("Cannot compare {:?} >= {:?}", left, right)),
        };
        Ok(Value::Bool(result))
    }

    /// Compare two values with not-equal
    pub fn neq(left: &Value, right: &Value) -> Result<Value, String> {
        let result = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a != b,
            (Value::Bool(a), Value::Bool(b)) => a != b,
            (Value::Float(a), Value::Float(b)) => a != b,
            (Value::Int(a), Value::Float(b)) => *a as f64 != *b,
            (Value::Float(a), Value::Int(b)) => *a != *b as f64,
            (Value::Nil, Value::Nil) => false,
            _ => true,
        };
        Ok(Value::Bool(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_ints() {
        let result = BinOpCompiler::add(&Value::Int(1), &Value::Int(2));
        assert_eq!(result, Ok(Value::Int(3)));
    }

    #[test]
    fn test_add_floats() {
        let result = BinOpCompiler::add(&Value::Float(1.5), &Value::Float(2.5));
        assert_eq!(result, Ok(Value::Float(4.0)));
    }

    #[test]
    fn test_add_mixed() {
        let result = BinOpCompiler::add(&Value::Int(1), &Value::Float(2.5));
        assert_eq!(result, Ok(Value::Float(3.5)));
    }

    #[test]
    fn test_sub_ints() {
        let result = BinOpCompiler::sub(&Value::Int(5), &Value::Int(3));
        assert_eq!(result, Ok(Value::Int(2)));
    }

    #[test]
    fn test_mul_ints() {
        let result = BinOpCompiler::mul(&Value::Int(3), &Value::Int(4));
        assert_eq!(result, Ok(Value::Int(12)));
    }

    #[test]
    fn test_div_ints() {
        let result = BinOpCompiler::div(&Value::Int(10), &Value::Int(2));
        assert_eq!(result, Ok(Value::Int(5)));
    }

    #[test]
    fn test_div_by_zero() {
        let result = BinOpCompiler::div(&Value::Int(1), &Value::Int(0));
        assert!(result.is_err());
    }

    #[test]
    fn test_lt_true() {
        let result = BinOpCompiler::lt(&Value::Int(1), &Value::Int(2));
        assert_eq!(result, Ok(Value::Bool(true)));
    }

    #[test]
    fn test_lt_false() {
        let result = BinOpCompiler::lt(&Value::Int(2), &Value::Int(1));
        assert_eq!(result, Ok(Value::Bool(false)));
    }

    #[test]
    fn test_eq_ints() {
        let result = BinOpCompiler::eq(&Value::Int(5), &Value::Int(5));
        assert_eq!(result, Ok(Value::Bool(true)));
    }

    #[test]
    fn test_eq_nils() {
        let result = BinOpCompiler::eq(&Value::Nil, &Value::Nil);
        assert_eq!(result, Ok(Value::Bool(true)));
    }

    #[test]
    fn test_neq_ints() {
        let result = BinOpCompiler::neq(&Value::Int(1), &Value::Int(2));
        assert_eq!(result, Ok(Value::Bool(true)));
    }
}
