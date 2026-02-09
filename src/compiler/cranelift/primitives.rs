// Primitive type compilation for Cranelift
//
// Handles compilation of Elle's primitive types (Int, Float, Bool, Nil)
// to Cranelift IR values.

use crate::value::Value;
use cranelift::prelude::*;

/// Represents a compiled Elle value in Cranelift IR
#[derive(Debug, Clone)]
pub enum CompiledValue {
    /// Nil value (represented as 0i64)
    Nil,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Float value (stored as bits in i64)
    Float(f64),
    /// A Cranelift value (intermediate representation)
    CraneliftValue(Value),
}

impl CompiledValue {
    /// Encode a boolean as Cranelift integer
    pub fn encode_bool(value: bool) -> i64 {
        if value {
            1
        } else {
            0
        }
    }

    /// Encode a nil value
    pub fn encode_nil() -> i64 {
        0
    }

    /// Encode an integer
    pub fn encode_int(value: i64) -> i64 {
        value
    }

    /// Encode a float as bits
    pub fn encode_float(value: f64) -> i64 {
        value.to_bits() as i64
    }

    /// Decode a float from bits
    pub fn decode_float(bits: i64) -> f64 {
        f64::from_bits(bits as u64)
    }
}

/// Type encoding for Elle values in Cranelift
///
/// Since we're passing Value by reference, we primarily need to
/// handle the reference/pointer representation. However, for
/// optimization, we can inline small values like integers.
pub struct PrimitiveEncoder;

impl PrimitiveEncoder {
    /// Get the Cranelift type for a primitive
    pub fn get_type(val: &Value) -> Option<Type> {
        match val {
            Value::Nil | Value::Bool(_) | Value::Int(_) => Some(types::I64),
            Value::Float(_) => Some(types::F64),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_bool_true() {
        assert_eq!(CompiledValue::encode_bool(true), 1);
    }

    #[test]
    fn test_encode_bool_false() {
        assert_eq!(CompiledValue::encode_bool(false), 0);
    }

    #[test]
    fn test_encode_nil() {
        assert_eq!(CompiledValue::encode_nil(), 0);
    }

    #[test]
    fn test_encode_decode_float() {
        let original = std::f64::consts::PI;
        let encoded = CompiledValue::encode_float(original);
        let decoded = CompiledValue::decode_float(encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_int() {
        assert_eq!(CompiledValue::encode_int(42), 42);
        assert_eq!(CompiledValue::encode_int(-1), -1);
        assert_eq!(CompiledValue::encode_int(i64::MAX), i64::MAX);
    }

    #[test]
    fn test_get_type_primitives() {
        assert_eq!(PrimitiveEncoder::get_type(&Value::Nil), Some(types::I64));
        assert_eq!(
            PrimitiveEncoder::get_type(&Value::Bool(true)),
            Some(types::I64)
        );
        assert_eq!(
            PrimitiveEncoder::get_type(&Value::Int(42)),
            Some(types::I64)
        );
        assert_eq!(
            PrimitiveEncoder::get_type(&Value::Float(std::f64::consts::PI)),
            Some(types::F64)
        );
    }
}
