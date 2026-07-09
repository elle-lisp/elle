//! Math ops that don't fit the unary-float mold: variadic/binary ops,
//! IEEE-754 constants, and f32 bitcasts.
//!
//! These need bespoke argument handling (optional base for `log`, two operands
//! for `pow`/`fmod`/`atan2`, division-by-zero guards, integer bit patterns), so
//! they live apart from the uniform `unary` handlers. `require_number` is shared
//! here because every one of them coerces at least one operand the same way.

use super::*;
use std::f64::consts::{E, PI};

/// Extract a single numeric arg as f64, or return a type error.
fn require_number(name: &str, v: &Value, ctx: &mut NativeCtx) -> Result<f64, (SignalBits, Value)> {
    v.as_number().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{name}: expected number, got {}", v.type_name()),
            ),
        )
    })
}

pub(super) fn prim_log(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = match require_number("log", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if args.len() == 1 {
        (SIG_OK, Value::float(value.ln()))
    } else {
        let base = match require_number("log", &args[1], ctx) {
            Ok(v) => v,
            Err(e) => return e,
        };
        (SIG_OK, Value::float(value.log(base)))
    }
}

pub(super) fn prim_pow(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
                ctx.error(
                    "type-error",
                    format!("pow: expected number, got {}", args[0].type_name()),
                ),
            ),
        }
    }
}

pub(super) fn prim_fmod(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = match require_number("fmod", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = match require_number("fmod", &args[1], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if b == 0.0 {
        return (
            SIG_ERROR,
            ctx.error("division-by-zero", "fmod: division by zero"),
        );
    }
    (SIG_OK, Value::float(a % b))
}

pub(super) fn prim_atan2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let y = match require_number("atan2", &args[0], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let x = match require_number("atan2", &args[1], ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    (SIG_OK, Value::float(y.atan2(x)))
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub(super) fn prim_pi(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(PI))
}
pub(super) fn prim_e(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(E))
}
pub(super) fn prim_inf(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::INFINITY))
}
pub(super) fn prim_neg_inf(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NEG_INFINITY))
}
pub(super) fn prim_nan(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::float(f64::NAN))
}

// ---------------------------------------------------------------------------
// IEEE 754 bitcast
// ---------------------------------------------------------------------------

pub(super) fn prim_f32_bits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match require_number("math/f32-bits", &args[0], ctx) {
        Ok(f) => (SIG_OK, Value::int((f as f32).to_bits() as i64)),
        Err(e) => e,
    }
}

pub(super) fn prim_f32_from_bits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(i) => (SIG_OK, Value::float(f32::from_bits(i as u32) as f64)),
        None => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "math/f32-from-bits: expected int, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}
