//! Bitwise operation primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Bitwise AND: fold all arguments with &
pub fn prim_bit_and(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/and: expected at least 2 arguments, got {}", args.len()),
            ),
        );
    }

    let Some(mut result) = args[0].as_int() else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bit/and: expected integer, got {}", args[0].type_name()),
            ),
        );
    };

    for arg in &args[1..] {
        let Some(n) = arg.as_int() else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("bit/and: expected integer, got {}", arg.type_name()),
                ),
            );
        };
        result &= n;
    }
    (SIG_OK, Value::int(result))
}

/// Bitwise OR: fold all arguments with |
pub fn prim_bit_or(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/or: expected at least 2 arguments, got {}", args.len()),
            ),
        );
    }

    let Some(mut result) = args[0].as_int() else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bit/or: expected integer, got {}", args[0].type_name()),
            ),
        );
    };

    for arg in &args[1..] {
        let Some(n) = arg.as_int() else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("bit/or: expected integer, got {}", arg.type_name()),
                ),
            );
        };
        result |= n;
    }
    (SIG_OK, Value::int(result))
}

/// Bitwise XOR: fold all arguments with ^
pub fn prim_bit_xor(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/xor: expected at least 2 arguments, got {}", args.len()),
            ),
        );
    }

    let Some(mut result) = args[0].as_int() else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bit/xor: expected integer, got {}", args[0].type_name()),
            ),
        );
    };

    for arg in &args[1..] {
        let Some(n) = arg.as_int() else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("bit/xor: expected integer, got {}", arg.type_name()),
                ),
            );
        };
        result ^= n;
    }
    (SIG_OK, Value::int(result))
}

/// Bitwise NOT: apply ! to single integer argument
pub fn prim_bit_not(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/not: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(!n)),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bit/not: expected integer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Left shift: shift first argument left by second argument (clamped to 0-63)
pub fn prim_bit_shift_left(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/shift-left: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let value = match args[0].as_int() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "bit/shift-left: expected integer, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let shift = match args[1].as_int() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "bit/shift-left: expected integer, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    if shift < 0 {
        return (
            SIG_ERROR,
            error_val("error", "bit/shift-left: shift amount must be non-negative"),
        );
    }

    // Clamp shift to 0-63
    let shift = (shift as u32).min(63);
    (SIG_OK, Value::int(value.wrapping_shl(shift)))
}

/// Arithmetic right shift: shift first argument right by second argument (clamped to 0-63)
pub fn prim_bit_shift_right(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bit/shift-right: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let value = match args[0].as_int() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "bit/shift-right: expected integer, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let shift = match args[1].as_int() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "bit/shift-right: expected integer, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    if shift < 0 {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "bit/shift-right: shift amount must be non-negative",
            ),
        );
    }

    // Clamp shift to 0-63
    let shift = (shift as u32).min(63);
    (SIG_OK, Value::int(value.wrapping_shr(shift)))
}

/// Declarative primitive definitions for bitwise functions.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bit/and",
        func: prim_bit_and,
        effect: Effect::none(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise AND of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/and 12 10) ;=> 8",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/or",
        func: prim_bit_or,
        effect: Effect::none(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise OR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/or 12 10) ;=> 14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/xor",
        func: prim_bit_xor,
        effect: Effect::none(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise XOR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/xor 12 10) ;=> 6",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/not",
        func: prim_bit_not,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Bitwise NOT of argument",
        params: &["x"],
        category: "bit",
        example: "(bit/not 0) ;=> -1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/shl",
        func: prim_bit_shift_left,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Left shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shl 1 3) ;=> 8",
        aliases: &["bit/shift-left"],
    },
    PrimitiveDef {
        name: "bit/shr",
        func: prim_bit_shift_right,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Arithmetic right shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shr 8 2) ;=> 2",
        aliases: &["bit/shift-right"],
    },
];
