use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
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
