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
        // Re-throw the condition
        let new_cond = Condition::new(cond.exception_id, cond.message().unwrap_or("").to_string());
        Err(new_cond)
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
        // Convert new Condition to old Condition
        let mut old_cond = crate::value_old::Condition::new(cond.exception_id);
        // Store message in old condition's FIELD_MESSAGE
        old_cond.set_field(
            crate::value_old::Condition::FIELD_MESSAGE,
            crate::value_old::Value::String(cond.message.clone().into()),
        );
        for (field_id, value) in cond.fields {
            let old_value = crate::primitives::coroutines::new_value_to_old(value);
            old_cond.set_field(field_id, old_value);
        }
        if let Some(bt) = cond.backtrace {
            old_cond.backtrace = Some(bt);
        }
        if let Some(loc) = cond.location {
            old_cond.location = Some(loc);
        }
        Ok(alloc(HeapObject::Condition(old_cond)))
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
        if let Some(msg) = cond.message() {
            Ok(Value::string(msg))
        } else {
            Ok(Value::NIL)
        }
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
            Some(data) => {
                // Convert old Value to new Value
                let new_value = crate::compiler::cps::primitives::old_value_to_new(data);
                Ok(new_value)
            }
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(format!(
            "exception-data: expected condition, got {}",
            args[0].type_name()
        )))
    }
}
