//! Exception introspection primitives (Phase 8)
//! Provides functions for handlers to query exception details
use crate::error::LResult;
use crate::value::Value;

/// Get the exception ID from a Condition
pub fn prim_exception_id(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("exception-id expects 1 argument".to_string().into());
    }

    match &args[0] {
        Value::Condition(cond) => Ok(Value::Int(cond.exception_id as i64)),
        _ => Err("exception-id expects a Condition".to_string().into()),
    }
}

/// Get a field value from a Condition by field ID
pub fn prim_condition_field(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err("condition-field expects 2 arguments (condition field-id)"
            .to_string()
            .into());
    }

    let field_id = args[1].as_int()? as u32;
    match &args[0] {
        Value::Condition(cond) => match cond.fields.get(&field_id) {
            Some(val) => Ok(val.clone()),
            None => Ok(Value::Nil),
        },
        _ => Err("condition-field expects a Condition as first argument"
            .to_string()
            .into()),
    }
}

/// Check if a Condition matches a given exception type ID
pub fn prim_condition_matches_type(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err("condition-matches-type expects 2 arguments"
            .to_string()
            .into());
    }

    let exception_type_id = args[1].as_int()? as u32;
    match &args[0] {
        Value::Condition(cond) => {
            use crate::vm::is_exception_subclass;
            Ok(Value::Bool(is_exception_subclass(
                cond.exception_id,
                exception_type_id,
            )))
        }
        _ => Err(
            "condition-matches-type expects a Condition as first argument"
                .to_string()
                .into(),
        ),
    }
}

/// Get the backtrace from a Condition
pub fn prim_condition_backtrace(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("condition-backtrace expects 1 argument".to_string().into());
    }

    use std::rc::Rc;
    match &args[0] {
        Value::Condition(cond) => match &cond.backtrace {
            Some(bt) => Ok(Value::String(Rc::from(bt.clone()))),
            None => Ok(Value::Nil),
        },
        _ => Err("condition-backtrace expects a Condition".to_string().into()),
    }
}
