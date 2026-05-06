//! Bitwise operation primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Coerce a value to i64 for bitwise operations.
/// Accepts integers directly and truncates finite floats.
/// Rejects NaN, infinity, and non-numeric types.
fn coerce_to_int(val: &Value, name: &str) -> Result<i64, (SignalBits, Value)> {
    if let Some(n) = val.as_int() {
        return Ok(n);
    }
    if let Some(f) = val.as_float() {
        if !f.is_finite() {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: cannot convert non-finite float to integer", name),
                ),
            ));
        }
        return Ok(f as i64);
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected number, got {}", name, val.type_name()),
        ),
    ))
}

/// Fold arguments with a bitwise operation.
fn fold_bitwise(args: &[Value], name: &str, op: fn(i64, i64) -> i64) -> (SignalBits, Value) {
    let mut result = match coerce_to_int(&args[0], name) {
        Ok(n) => n,
        Err(e) => return e,
    };
    for arg in &args[1..] {
        let n = match coerce_to_int(arg, name) {
            Ok(n) => n,
            Err(e) => return e,
        };
        result = op(result, n);
    }
    (SIG_OK, Value::int(result))
}

pub(crate) fn prim_bit_and(args: &[Value]) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/and", |a, b| a & b)
}

pub(crate) fn prim_bit_or(args: &[Value]) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/or", |a, b| a | b)
}

pub(crate) fn prim_bit_xor(args: &[Value]) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/xor", |a, b| a ^ b)
}

/// Bitwise NOT: apply ! to single integer argument
pub(crate) fn prim_bit_not(args: &[Value]) -> (SignalBits, Value) {
    match coerce_to_int(&args[0], "bit/not") {
        Ok(n) => (SIG_OK, Value::int(!n)),
        Err(e) => e,
    }
}

/// Left shift: shift first argument left by second argument (clamped to 0-63)
pub(crate) fn prim_bit_shift_left(args: &[Value]) -> (SignalBits, Value) {
    let value = match coerce_to_int(&args[0], "bit/shift-left") {
        Ok(v) => v,
        Err(e) => return e,
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
            error_val(
                "argument-error",
                "bit/shift-left: shift amount must be non-negative",
            ),
        );
    }

    // Clamp shift to 0-63
    let shift = (shift as u32).min(63);
    (SIG_OK, Value::int(value.wrapping_shl(shift)))
}

/// Arithmetic right shift: shift first argument right by second argument (clamped to 0-63)
pub(crate) fn prim_bit_shift_right(args: &[Value]) -> (SignalBits, Value) {
    let value = match coerce_to_int(&args[0], "bit/shift-right") {
        Ok(v) => v,
        Err(e) => return e,
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
                "argument-error",
                "bit/shift-right: shift amount must be non-negative",
            ),
        );
    }

    // Clamp shift to 0-63
    let shift = (shift as u32).min(63);
    (SIG_OK, Value::int(value.wrapping_shr(shift)))
}

/// Declarative primitive definitions for bitwise functions.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bit/and",
        func: prim_bit_and,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise AND of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/and 12 10) #=> 8",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/or",
        func: prim_bit_or,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise OR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/or 12 10) #=> 14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/xor",
        func: prim_bit_xor,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise XOR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/xor 12 10) #=> 6",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/not",
        func: prim_bit_not,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Bitwise NOT of argument",
        params: &["x"],
        category: "bit",
        example: "(bit/not 0) #=> -1",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bit/shl",
        func: prim_bit_shift_left,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Left shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shl 1 3) #=> 8",
        aliases: &["bit/shift-left"],
    },
    PrimitiveDef {
        name: "bit/shr",
        func: prim_bit_shift_right,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Arithmetic right shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shr 8 2) #=> 2",
        aliases: &["bit/shift-right"],
    },
];
