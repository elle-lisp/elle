//! Exception handling primitives
use crate::value::{Condition, Value};

/// Throw an exception
pub fn prim_throw(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "throw: expected at least 1 argument, got 0".to_string(),
        ));
    }

    if let Some(msg) = args[0].as_string() {
        Err(Condition::error(msg.to_string()))
    } else if let Some(cond) = args[0].as_condition() {
        // Re-throw the condition - clone it
        Err(cond.clone())
    } else {
        Err(Condition::type_error(format!(
            "throw: expected string or condition, got {}",
            args[0].type_name()
        )))
    }
}

/// Create an exception
pub fn prim_exception(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "exception: expected at least 1 argument, got 0".to_string(),
        ));
    }

    if let Some(msg) = args[0].as_string() {
        let cond = if args.len() > 1 {
            Condition::generic_with_data(msg.to_string(), args[1])
        } else {
            Condition::generic(msg.to_string())
        };
        use crate::value::heap::{alloc, HeapObject};
        // Store the Condition directly (no conversion needed)
        Ok(alloc(HeapObject::Condition(cond)))
    } else {
        Err(Condition::type_error(
            "exception: expected string as first argument".to_string(),
        ))
    }
}

/// Get the message from an exception
pub fn prim_exception_message(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "exception-message: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(cond) = args[0].as_condition() {
        // message() returns &str directly (always present)
        Ok(Value::string(cond.message()))
    } else {
        Err(Condition::type_error(format!(
            "exception-message: expected condition, got {}",
            args[0].type_name()
        )))
    }
}

/// Get the data from an exception
pub fn prim_exception_data(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "exception-data: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(cond) = args[0].as_condition() {
        match cond.data() {
            Some(data) => Ok(*data),
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(format!(
            "exception-data: expected condition, got {}",
            args[0].type_name()
        )))
    }
}
