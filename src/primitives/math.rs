use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::f64::consts::{E, PI};

pub(crate) fn prim_sqrt(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_sin(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_cos(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_tan(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_log(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_exp(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_pow(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pow: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    if let (Some(a), Some(b)) = (args[0].as_int(), args[1].as_int()) {
        if b < 0 {
            (SIG_OK, Value::float((a as f64).powf(b as f64)))
        } else {
            (SIG_OK, Value::int(a.pow(b as u32)))
        }
    } else {
        match (args[0].as_number(), args[1].as_number()) {
            (Some(a), Some(b)) => (SIG_OK, Value::float(a.powf(b))),
            _ => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("pow: expected number, got {}", args[0].type_name()),
                ),
            ),
        }
    }
}

pub(crate) fn prim_floor(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_ceil(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_round(args: &[Value]) -> (SignalBits, Value) {
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

pub(crate) fn prim_asin(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("asin: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).asin())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.asin())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("asin: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_acos(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("acos: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).acos())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.acos())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("acos: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_atan(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("atan: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).atan())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.atan())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("atan: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_atan2(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("atan2: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let y = match args[0].as_int() {
        Some(n) => n as f64,
        None => match args[0].as_float() {
            Some(f) => f,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("atan2: expected number, got {}", args[0].type_name()),
                    ),
                )
            }
        },
    };

    let x = match args[1].as_int() {
        Some(n) => n as f64,
        None => match args[1].as_float() {
            Some(f) => f,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("atan2: expected number, got {}", args[1].type_name()),
                    ),
                )
            }
        },
    };

    (SIG_OK, Value::float(y.atan2(x)))
}

pub(crate) fn prim_sinh(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sinh: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).sinh())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.sinh())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sinh: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_cosh(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("cosh: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).cosh())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.cosh())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("cosh: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_tanh(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("tanh: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).tanh())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.tanh())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("tanh: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_log2(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("log2: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).log2())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.log2())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("log2: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_log10(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("log10: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).log10())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.log10())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("log10: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_trunc(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("trunc: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).trunc())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.trunc())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("trunc: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_cbrt(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("cbrt: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).cbrt())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.cbrt())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("cbrt: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_exp2(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("exp2: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float((n as f64).exp2())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f.exp2())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("exp2: expected number, got {}", args[0].type_name()),
                ),
            ),
        },
    }
}

pub(crate) fn prim_pi(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(PI))
}

pub(crate) fn prim_e(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(E))
}

pub(crate) fn prim_inf(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::INFINITY))
}

pub(crate) fn prim_neg_inf(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NEG_INFINITY))
}

pub(crate) fn prim_nan(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NAN))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "math/sqrt",
        func: prim_sqrt,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::silent(),
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
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "The mathematical constant e (Euler's number).",
        params: &[],
        category: "math",
        example: "(math/e)",
        aliases: &["e"],
    },
    PrimitiveDef {
        name: "math/asin",
        func: prim_asin,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arcsine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/asin 1)",
        aliases: &["asin"],
    },
    PrimitiveDef {
        name: "math/acos",
        func: prim_acos,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arccosine of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/acos 1)",
        aliases: &["acos"],
    },
    PrimitiveDef {
        name: "math/atan",
        func: prim_atan,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the arctangent of a number (in radians).",
        params: &["x"],
        category: "math",
        example: "(math/atan 1)",
        aliases: &["atan"],
    },
    PrimitiveDef {
        name: "math/atan2",
        func: prim_atan2,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Returns the arctangent of y/x (in radians), using the signs of both arguments to determine the quadrant.",
        params: &["y", "x"],
        category: "math",
        example: "(math/atan2 1 1)",
        aliases: &["atan2"],
    },
    PrimitiveDef {
        name: "math/sinh",
        func: prim_sinh,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic sine of a number.",
        params: &["x"],
        category: "math",
        example: "(math/sinh 1)",
        aliases: &["sinh"],
    },
    PrimitiveDef {
        name: "math/cosh",
        func: prim_cosh,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic cosine of a number.",
        params: &["x"],
        category: "math",
        example: "(math/cosh 1)",
        aliases: &["cosh"],
    },
    PrimitiveDef {
        name: "math/tanh",
        func: prim_tanh,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the hyperbolic tangent of a number.",
        params: &["x"],
        category: "math",
        example: "(math/tanh 1)",
        aliases: &["tanh"],
    },
    PrimitiveDef {
        name: "math/log2",
        func: prim_log2,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the base-2 logarithm of a number.",
        params: &["x"],
        category: "math",
        example: "(math/log2 8)",
        aliases: &["log2"],
    },
    PrimitiveDef {
        name: "math/log10",
        func: prim_log10,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the base-10 logarithm of a number.",
        params: &["x"],
        category: "math",
        example: "(math/log10 100)",
        aliases: &["log10"],
    },
    PrimitiveDef {
        name: "math/trunc",
        func: prim_trunc,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the integer part of a number, truncating toward zero.",
        params: &["x"],
        category: "math",
        example: "(math/trunc 3.7)",
        aliases: &["trunc"],
    },
    PrimitiveDef {
        name: "math/cbrt",
        func: prim_cbrt,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns the cube root of a number.",
        params: &["x"],
        category: "math",
        example: "(math/cbrt 27)",
        aliases: &["cbrt"],
    },
    PrimitiveDef {
        name: "math/exp2",
        func: prim_exp2,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns 2 raised to the power of x.",
        params: &["x"],
        category: "math",
        example: "(math/exp2 3)",
        aliases: &["exp2"],
    },
    PrimitiveDef {
        name: "math/inf",
        func: prim_inf,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Positive infinity (IEEE 754).",
        params: &[],
        category: "math",
        example: "(math/inf)",
        aliases: &["+inf", "inf"],
    },
    PrimitiveDef {
        name: "math/-inf",
        func: prim_neg_inf,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Negative infinity (IEEE 754).",
        params: &[],
        category: "math",
        example: "(math/-inf)",
        aliases: &["-inf"],
    },
    PrimitiveDef {
        name: "math/nan",
        func: prim_nan,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Not-a-number (IEEE 754 NaN).",
        params: &[],
        category: "math",
        example: "(math/nan)",
        aliases: &["nan"],
    },
];
