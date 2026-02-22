//! Comparison primitives
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Equality comparison
pub fn prim_eq(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("=: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        if args[0] == args[1] {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Less than comparison
pub fn prim_lt(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("<: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a < b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a < b,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("<: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };
    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Greater than comparison
pub fn prim_gt(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(">: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a > b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a > b,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(">: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };
    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Less than or equal comparison
pub fn prim_le(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("<=: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a <= b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a <= b,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("<=: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };
    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Greater than or equal comparison
pub fn prim_ge(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(">=: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let result = match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => a >= b,
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => a >= b,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(">=: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };
    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}
