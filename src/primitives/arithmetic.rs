use crate::arithmetic;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

pub(crate) fn prim_abs(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("abs: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match arithmetic::abs_value(&args[0]) {
        Ok(val) => (SIG_OK, val),
        Err(err_val) => (SIG_ERROR, err_val),
    }
}

pub(crate) fn prim_min(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "min: expected at least 1 argument, got 0"),
        );
    }

    let mut min = args[0];
    for arg in &args[1..] {
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("min: expected number, got {}", arg.type_name()),
                ),
            );
        }
        min = arithmetic::min_values(&min, arg);
    }
    (SIG_OK, min)
}

pub(crate) fn prim_max(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val("arity-error", "max: expected at least 1 argument, got 0"),
        );
    }

    let mut max = args[0];
    for arg in &args[1..] {
        if !arg.is_number() {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("max: expected number, got {}", arg.type_name()),
                ),
            );
        }
        max = arithmetic::max_values(&max, arg);
    }
    (SIG_OK, max)
}

pub(crate) fn prim_even(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("even?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::bool(n % 2 == 0)),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("even?: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

pub(crate) fn prim_odd(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("odd?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::bool(n % 2 != 0)),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("odd?: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "abs",
        func: prim_abs,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Absolute value.",
        params: &["x"],
        category: "arithmetic",
        example: "(abs -5) #=> 5\n(abs 3) #=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "min",
        func: prim_min,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Minimum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(min 3 1 4) #=> 1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "max",
        func: prim_max,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Maximum of all arguments.",
        params: &["xs"],
        category: "arithmetic",
        example: "(max 3 1 4) #=> 4",
        aliases: &[],
    },
    PrimitiveDef {
        name: "even?",
        func: prim_even,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Test if integer is even.",
        params: &["n"],
        category: "arithmetic",
        example: "(even? 4) #=> true\n(even? 3) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "odd?",
        func: prim_odd,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Test if integer is odd.",
        params: &["n"],
        category: "arithmetic",
        example: "(odd? 3) #=> true\n(odd? 4) #=> false",
        aliases: &[],
    },
];
