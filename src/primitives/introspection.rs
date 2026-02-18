//! Exception introspection primitives (Phase 8)
//! Provides functions for handlers to query exception details
use crate::value::{Condition, Value};

/// Get the exception ID from a Condition
pub fn prim_exception_id(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "exception-id: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(cond) = args[0].as_condition() {
        Ok(Value::int(cond.exception_id as i64))
    } else {
        Err(Condition::type_error(
            "exception-id: expected a Condition".to_string(),
        ))
    }
}

/// Get a field value from a Condition by field ID
pub fn prim_condition_field(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "condition-field: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let field_id = args[1].as_int().ok_or_else(|| {
        Condition::type_error("condition-field: field-id must be an integer".to_string())
    })? as u32;
    if let Some(cond) = args[0].as_condition() {
        match cond.fields.get(&field_id) {
            Some(val) => {
                // Convert old Value to new Value
                let new_value = crate::primitives::coroutines::old_value_to_new(val);
                Ok(new_value)
            }
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(
            "condition-field: expected a Condition as first argument".to_string(),
        ))
    }
}

/// Check if a Condition matches a given exception type ID
pub fn prim_condition_matches_type(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "condition-matches-type: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let exception_type_id = args[1].as_int().ok_or_else(|| {
        Condition::type_error(
            "condition-matches-type: exception-type-id must be an integer".to_string(),
        )
    })? as u32;
    if let Some(cond) = args[0].as_condition() {
        use crate::vm::is_exception_subclass;
        Ok(Value::bool(is_exception_subclass(
            cond.exception_id,
            exception_type_id,
        )))
    } else {
        Err(Condition::type_error(
            "condition-matches-type: expected a Condition as first argument".to_string(),
        ))
    }
}

/// Get the backtrace from a Condition
pub fn prim_condition_backtrace(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "condition-backtrace: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(cond) = args[0].as_condition() {
        match &cond.backtrace {
            Some(bt) => Ok(Value::string(bt.as_str())),
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(
            "condition-backtrace: expected a Condition".to_string(),
        ))
    }
}
