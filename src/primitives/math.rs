use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::f64::consts::{E, PI};

pub fn prim_sqrt(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sqrt: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).sqrt())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.sqrt())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sqrt: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_sin(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sin: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).sin())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.sin())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sin: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_cos(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("cos: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).cos())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.cos())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("cos: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_tan(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("tan: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).tan())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.tan())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("tan: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_log(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("log: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let value = match args[0].as_int() {
        Some(n) => n as f64,
        None => match args[0].as_float() {
            Some(f) => f,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("log: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };

    if args.len() == 1 {
        // Natural logarithm
        (SIG_OK, Value::float(value.ln()))
    } else {
        // Logarithm with specified base
        let base = match args[1].as_int() {
            Some(n) => n as f64,
            None => match args[1].as_float() {
                Some(f) => f,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("log: expected number, got {}", args[1].type_name()),
                        ),
                    )
                }
            },
        };
        (SIG_OK, Value::float(value.log(base)))
    }
}

pub fn prim_exp(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("exp: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).exp())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.exp())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("exp: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_pow(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pow: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            if b < 0 {
                (SIG_OK, Value::float((a as f64).powf(b as f64)))
            } else {
                (SIG_OK, Value::int(a.pow(b as u32)))
            }
        }
        _ => match (args[0].as_float(), args[1].as_float()) {
            (Some(a), Some(b)) => (SIG_OK, Value::float(a.powf(b))),
            _ => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("pow: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_floor(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("floor: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::int(f.floor() as i64)),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("floor: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_ceil(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("ceil: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::int(f.ceil() as i64)),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("ceil: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_round(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("round: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::int(f.round() as i64)),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("round: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub fn prim_pi(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(PI))
}

pub fn prim_e(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(E))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "math/sqrt",
        func: prim_sqrt,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the square root of a number.",
        params: &["x"],
        category: "math",
        example: "(math/sqrt 16)",
        aliases: &["sqrt"],
    },
    PrimitiveDef {
        name: "math/sin",
        func: prim_sin,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the sine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/sin 0)",
        aliases: &["sin"],
    },
    PrimitiveDef {
        name: "math/cos",
        func: prim_cos,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the cosine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/cos 0)",
        aliases: &["cos"],
    },
    PrimitiveDef {
        name: "math/tan",
        func: prim_tan,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the tangent of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/tan 0)",
        aliases: &["tan"],
    },
    PrimitiveDef {
        name: "math/log",
        func: prim_log,
        effect: Effect::none(),
        arity: Arity::Range(1, 2),
        doc: "Returns the natural logarithm of x, or logarithm with specified base.",
        params: &["x", "base"],
        category: "math",
        example: "(math/log 2.718281828)",
        aliases: &["log"],
    },
    PrimitiveDef {
        name: "math/exp",
        func: prim_exp,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns e raised to the power of x.",
        params: &["x"],
        category: "math",
        example: "(math/exp 1)",
        aliases: &["exp"],
    },
    PrimitiveDef {
        name: "math/pow",
        func: prim_pow,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Returns x raised to the power of y.",
        params: &["x", "y"],
        category: "math",
        example: "(math/pow 2 8)",
        aliases: &["pow"],
    },
    PrimitiveDef {
        name: "math/floor",
        func: prim_floor,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the largest integer less than or equal to x.",
        params: &["x"],
        category: "math",
        example: "(math/floor 3.7)",
        aliases: &["floor"],
    },
    PrimitiveDef {
        name: "math/ceil",
        func: prim_ceil,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the smallest integer greater than or equal to x.",
        params: &["x"],
        category: "math",
        example: "(math/ceil 3.2)",
        aliases: &["ceil"],
    },
    PrimitiveDef {
        name: "math/round",
        func: prim_round,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns the nearest integer to x.",
        params: &["x"],
        category: "math",
        example: "(math/round 3.5)",
        aliases: &["round"],
    },
    PrimitiveDef {
        name: "math/pi",
        func: prim_pi,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "The mathematical constant pi (π).",
        params: &[],
        category: "math",
        example: "(math/pi)",
        aliases: &["pi"],
    },
    PrimitiveDef {
        name: "math/e",
        func: prim_e,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "The mathematical constant e (Euler's number).",
        params: &[],
        category: "math",
        example: "(math/e)",
        aliases: &["e"],
    },
];
