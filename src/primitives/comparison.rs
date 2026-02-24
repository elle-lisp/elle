//! Comparison primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
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

/// Declarative primitive definitions for comparison functions.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "=",
        func: prim_eq,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Test equality of two values",
        params: &["a", "b"],
        category: "comparison",
        example: "(= 1 1)",
        aliases: &["eq?"],
    },
    PrimitiveDef {
        name: "<",
        func: prim_lt,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Test if first number is less than second",
        params: &["a", "b"],
        category: "comparison",
        example: "(< 1 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: ">",
        func: prim_gt,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Test if first number is greater than second",
        params: &["a", "b"],
        category: "comparison",
        example: "(> 2 1)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "<=",
        func: prim_le,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Test if first number is less than or equal to second",
        params: &["a", "b"],
        category: "comparison",
        example: "(<= 1 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: ">=",
        func: prim_ge,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Test if first number is greater than or equal to second",
        params: &["a", "b"],
        category: "comparison",
        example: "(>= 2 1)",
        aliases: &[],
    },
];
