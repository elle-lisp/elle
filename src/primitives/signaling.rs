//! Signaling primitives for the condition system
//!
//! Provides signal, warn, and error functions for the condition system.

use crate::value::condition::exception_name;
use crate::value::{Condition, Value};

/// Signal a condition (silent - just propagates)
pub fn prim_signal(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "signal: expected at least 1 argument (condition ID), got 0".to_string(),
        ));
    }

    // First arg should be the exception ID
    if let Some(id) = args[0].as_int() {
        if id < 0 || id > u32::MAX as i64 {
            return Err(Condition::error(format!(
                "signal: invalid exception ID: {}",
                id
            )));
        }

        let exception_id = id as u32;
        let msg = format!("signaled {}", exception_name(exception_id));
        let mut condition = Condition::new(exception_id, msg);

        // Remaining args are field values
        // For now, we'll store them as positional fields
        for (i, field_value) in args[1..].iter().enumerate() {
            condition.set_field(i as u32, *field_value);
        }

        use crate::value::heap::{alloc, HeapObject};
        // Store the Condition directly (no conversion needed)
        Ok(alloc(HeapObject::Condition(condition)))
    } else {
        Err(Condition::type_error(
            "signal: first argument must be an integer (exception ID)".to_string(),
        ))
    }
}

/// Warn about a condition (prints if unhandled)
pub fn prim_warn(args: &[Value]) -> Result<Value, Condition> {
    // Same as signal for now - actual warning behavior would be in the handler
    prim_signal(args)
}

/// Signal an error condition (goes to debugger if unhandled)
pub fn prim_error(args: &[Value]) -> Result<Value, Condition> {
    // Same as signal for now - actual error behavior would be in the handler
    prim_signal(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creates_condition() {
        let result = prim_signal(&[Value::int(1)]).unwrap();
        if let Some(cond) = result.as_condition() {
            assert_eq!(cond.exception_id, 1);
        } else {
            panic!("Expected Condition");
        }
    }

    #[test]
    fn test_signal_with_fields() {
        let result = prim_signal(&[Value::int(1), Value::int(42), Value::string("test")]).unwrap();
        if let Some(cond) = result.as_condition() {
            assert_eq!(cond.exception_id, 1);
            assert_eq!(cond.get_field(0), Some(&Value::int(42)));
            assert_eq!(cond.get_field(1), Some(&Value::string("test")));
        } else {
            panic!("Expected Condition");
        }
    }

    #[test]
    fn test_signal_invalid_id() {
        let result = prim_signal(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_warn_same_as_signal() {
        let sig = prim_signal(&[Value::int(2)]).unwrap();
        let warn = prim_warn(&[Value::int(2)]).unwrap();
        assert_eq!(sig, warn);
    }
}
