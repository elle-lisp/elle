//! Signaling primitives for the condition system
//!
//! Provides signal, warn, and error functions for the condition system.

use crate::error::LResult;
use crate::value::{Condition, Value};
use std::rc::Rc;

/// Signal a condition (silent - just propagates)
pub fn prim_signal(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err("signal requires at least 1 argument (condition ID)"
            .to_string()
            .into());
    }

    // First arg should be the exception ID
    match &args[0] {
        Value::Int(id) => {
            if *id < 0 || *id > u32::MAX as i64 {
                return Err(format!("Invalid exception ID: {}", id).into());
            }

            let mut condition = Condition::new(*id as u32);

            // Remaining args are field values
            // For now, we'll store them as positional fields
            for (i, field_value) in args[1..].iter().enumerate() {
                condition.set_field(i as u32, field_value.clone());
            }

            Ok(Value::Condition(Rc::new(condition)))
        }
        _ => Err("signal: first argument must be an integer (exception ID)"
            .to_string()
            .into()),
    }
}

/// Warn about a condition (prints if unhandled)
pub fn prim_warn(args: &[Value]) -> LResult<Value> {
    // Same as signal for now - actual warning behavior would be in the handler
    prim_signal(args)
}

/// Signal an error condition (goes to debugger if unhandled)
pub fn prim_error(args: &[Value]) -> LResult<Value> {
    // Same as signal for now - actual error behavior would be in the handler
    prim_signal(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creates_condition() {
        let result = prim_signal(&[Value::Int(1)]).unwrap();
        match result {
            Value::Condition(cond) => {
                assert_eq!(cond.exception_id, 1);
            }
            _ => panic!("Expected Condition"),
        }
    }

    #[test]
    fn test_signal_with_fields() {
        let result =
            prim_signal(&[Value::Int(1), Value::Int(42), Value::String("test".into())]).unwrap();
        match result {
            Value::Condition(cond) => {
                assert_eq!(cond.exception_id, 1);
                assert_eq!(cond.get_field(0), Some(&Value::Int(42)));
                assert_eq!(cond.get_field(1), Some(&Value::String("test".into())));
            }
            _ => panic!("Expected Condition"),
        }
    }

    #[test]
    fn test_signal_invalid_id() {
        let result = prim_signal(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_warn_same_as_signal() {
        let sig = prim_signal(&[Value::Int(2)]).unwrap();
        let warn = prim_warn(&[Value::Int(2)]).unwrap();
        assert_eq!(sig, warn);
    }
}
