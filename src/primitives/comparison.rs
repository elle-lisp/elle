//! Comparison primitives
use crate::value::{Condition, Value};

/// Equality comparison
pub fn prim_eq(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "=: expected 2 arguments, got {}",
            args.len()
        )));
    }
    Ok(if args[0] == args[1] {
        Value::TRUE
    } else {
        Value::FALSE
    })
}

/// Less than comparison
pub fn prim_lt(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "<: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a < b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a < b,
            _ => {
                return Err(Condition::type_error(format!(
                    "<: expected number, got {}",
                    args[0].type_name()
                )))
            }
        },
    };
    Ok(if result { Value::TRUE } else { Value::FALSE })
}

/// Greater than comparison
pub fn prim_gt(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            ">: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a > b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a > b,
            _ => {
                return Err(Condition::type_error(format!(
                    ">: expected number, got {}",
                    args[0].type_name()
                )))
            }
        },
    };
    Ok(if result { Value::TRUE } else { Value::FALSE })
}

/// Less than or equal comparison
pub fn prim_le(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "<=: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a <= b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a <= b,
            _ => {
                return Err(Condition::type_error(format!(
                    "<=: expected number, got {}",
                    args[0].type_name()
                )))
            }
        },
    };
    Ok(if result { Value::TRUE } else { Value::FALSE })
}

/// Greater than or equal comparison
pub fn prim_ge(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            ">=: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a >= b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a >= b,
            _ => {
                return Err(Condition::type_error(format!(
                    ">=: expected number, got {}",
                    args[0].type_name()
                )))
            }
        },
    };
    Ok(if result { Value::TRUE } else { Value::FALSE })
}
