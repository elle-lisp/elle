//! Exception handling primitives
use crate::error::LResult;
use crate::value::{Exception, Value};
use std::rc::Rc;

/// Throw an exception
pub fn prim_throw(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err("throw requires at least 1 argument".to_string().into());
    }

    match &args[0] {
        Value::String(msg) => Err(msg.to_string().into()),
        Value::Exception(exc) => Err(exc.message.to_string().into()),
        other => Err(format!(
            "throw requires a string or exception, got {}",
            other.type_name()
        )
        .into()),
    }
}

/// Create an exception
pub fn prim_exception(args: &[Value]) -> LResult<Value> {
    if args.is_empty() {
        return Err("exception requires at least 1 argument".to_string().into());
    }

    match &args[0] {
        Value::String(msg) => {
            let exc = if args.len() > 1 {
                Exception::with_data(msg.to_string(), args[1].clone())
            } else {
                Exception::new(msg.to_string())
            };
            Ok(Value::Exception(Rc::new(exc)))
        }
        _ => Err("exception requires a string as first argument"
            .to_string()
            .into()),
    }
}

/// Get the message from an exception
pub fn prim_exception_message(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("exception-message requires exactly 1 argument"
            .to_string()
            .into());
    }

    match &args[0] {
        Value::Exception(exc) => Ok(Value::String(exc.message.clone())),
        _ => Err(format!(
            "exception-message requires an exception, got {}",
            args[0].type_name()
        )
        .into()),
    }
}

/// Get the data from an exception
pub fn prim_exception_data(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err("exception-data requires exactly 1 argument"
            .to_string()
            .into());
    }

    match &args[0] {
        Value::Exception(exc) => match &exc.data {
            Some(data) => Ok((**data).clone()),
            None => Ok(Value::Nil),
        },
        _ => Err(format!(
            "exception-data requires an exception, got {}",
            args[0].type_name()
        )
        .into()),
    }
}
