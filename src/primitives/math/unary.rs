//! Unary float math primitives (sqrt, trig, logs, …).
//!
//! Every op here is number → float with identical error handling, so each
//! reduces to one line delegating to `unary_float`. Keeping them together with
//! that helper makes the shared shape obvious and the divergent ops (log/pow/…)
//! clearly the exceptions in `special`.

use super::*;

/// Unary op: number → float (e.g. sqrt, sin, cos, …)
fn unary_float(
    name: &str,
    args: &[Value],
    op: fn(f64) -> f64,
    ctx: &mut NativeCtx,
) -> (SignalBits, Value) {
    match args[0].as_number() {
        Some(n) => (SIG_OK, Value::float(op(n))),
        None => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{name}: expected number, got {}", args[0].type_name()),
            ),
        ),
    }
}

pub(super) fn prim_sqrt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sqrt", args, f64::sqrt, ctx)
}
pub(super) fn prim_sin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sin", args, f64::sin, ctx)
}
pub(super) fn prim_cos(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cos", args, f64::cos, ctx)
}
pub(super) fn prim_tan(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("tan", args, f64::tan, ctx)
}
pub(super) fn prim_exp(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("exp", args, f64::exp, ctx)
}
pub(super) fn prim_asin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("asin", args, f64::asin, ctx)
}
pub(super) fn prim_acos(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("acos", args, f64::acos, ctx)
}
pub(super) fn prim_atan(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("atan", args, f64::atan, ctx)
}
pub(super) fn prim_sinh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("sinh", args, f64::sinh, ctx)
}
pub(super) fn prim_cosh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cosh", args, f64::cosh, ctx)
}
pub(super) fn prim_tanh(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("tanh", args, f64::tanh, ctx)
}
pub(super) fn prim_log2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("log2", args, f64::log2, ctx)
}
pub(super) fn prim_log10(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("log10", args, f64::log10, ctx)
}
pub(super) fn prim_trunc(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("trunc", args, f64::trunc, ctx)
}
pub(super) fn prim_cbrt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("cbrt", args, f64::cbrt, ctx)
}
pub(super) fn prim_exp2(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    unary_float("exp2", args, f64::exp2, ctx)
}
