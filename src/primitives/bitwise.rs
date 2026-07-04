//! Bitwise operation primitives
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Coerce a value to i64 for bitwise operations.
/// Accepts integers directly and truncates finite floats.
/// Rejects NaN, infinity, and non-numeric types.
fn coerce_to_int(val: &Value, name: &str, ctx: &mut NativeCtx) -> Result<i64, (SignalBits, Value)> {
    if let Some(n) = val.as_int() {
        return Ok(n);
    }
    if let Some(f) = val.as_float() {
        if !f.is_finite() {
            return Err((
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("{}: cannot convert non-finite float to integer", name),
                ),
            ));
        }
        return Ok(f as i64);
    }
    Err((
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("{}: expected number, got {}", name, val.type_name()),
        ),
    ))
}

/// Fold arguments with a bitwise operation.
fn fold_bitwise(
    args: &[Value],
    name: &str,
    op: fn(i64, i64) -> i64,
    ctx: &mut NativeCtx,
) -> (SignalBits, Value) {
    let mut result = match coerce_to_int(&args[0], name, ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };
    for arg in &args[1..] {
        let n = match coerce_to_int(arg, name, ctx) {
            Ok(n) => n,
            Err(e) => return e,
        };
        result = op(result, n);
    }
    (SIG_OK, Value::int(result))
}

pub(crate) fn prim_bit_and(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/and", |a, b| a & b, ctx)
}

pub(crate) fn prim_bit_or(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/or", |a, b| a | b, ctx)
}

pub(crate) fn prim_bit_xor(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    fold_bitwise(args, "bit/xor", |a, b| a ^ b, ctx)
}

/// Bitwise NOT: apply ! to single integer argument
pub(crate) fn prim_bit_not(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match coerce_to_int(&args[0], "bit/not", ctx) {
        Ok(n) => (SIG_OK, Value::int(!n)),
        Err(e) => e,
    }
}

/// Left shift: shift first argument left by second argument (clamped to 0-63)
pub(crate) fn prim_bit_shift_left(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = match coerce_to_int(&args[0], "bit/shift-left", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let shift = match args[1].as_int() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
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
            ctx.error(
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
pub(crate) fn prim_bit_shift_right(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = match coerce_to_int(&args[0], "bit/shift-right", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let shift = match args[1].as_int() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
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
            ctx.error(
                "argument-error",
                "bit/shift-right: shift amount must be non-negative",
            ),
        );
    }

    // Clamp shift to 0-63
    let shift = (shift as u32).min(63);
    (SIG_OK, Value::int(value.wrapping_shr(shift)))
}

// Declarative primitive definitions for bitwise functions.
primitive! {
    "bit/and" => prim_bit_and {
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise AND of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/and 12 10) #=> 8",
        effect: RegionEffect::Immediate,
    }
    "bit/or" => prim_bit_or {
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise OR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/or 12 10) #=> 14",
        effect: RegionEffect::Immediate,
    }
    "bit/xor" => prim_bit_xor {
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Bitwise XOR of all arguments",
        params: &["xs"],
        category: "bit",
        example: "(bit/xor 12 10) #=> 6",
        effect: RegionEffect::Immediate,
    }
    "bit/not" => prim_bit_not {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Bitwise NOT of argument",
        params: &["x"],
        category: "bit",
        example: "(bit/not 0) #=> -1",
        effect: RegionEffect::Immediate,
    }
    "bit/shl" => prim_bit_shift_left {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Left shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shl 1 3) #=> 8",
        aliases: &["bit/shift-left"],
        effect: RegionEffect::Immediate,
    }
    "bit/shr" => prim_bit_shift_right {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Arithmetic right shift first argument by second argument (clamped to 0-63).",
        params: &["x", "n"],
        category: "bit",
        example: "(bit/shr 8 2) #=> 2",
        aliases: &["bit/shift-right"],
        effect: RegionEffect::Immediate,
    }
}
