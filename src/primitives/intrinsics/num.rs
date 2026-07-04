use super::*;

pub(super) fn prim_add(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match arithmetic::add_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

pub(super) fn prim_sub(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.len() == 1 {
        // Unary negation. wrapping_neg: -i64::MIN wraps to i64::MIN
        // (bare `-` panics on overflow in debug builds).
        if let Some(n) = args[0].as_int() {
            return (SIG_OK, Value::int(n.wrapping_neg()));
        }
        if let Some(f) = args[0].as_float() {
            return (SIG_OK, Value::float(-f));
        }
        return type_err("%sub", "number", &args[0], ctx);
    }
    match arithmetic::sub_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

pub(super) fn prim_mul(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match arithmetic::mul_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

pub(super) fn prim_div(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match arithmetic::div_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

pub(super) fn prim_rem(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match arithmetic::remainder_values(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

pub(super) fn prim_mod(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Floored modulus: ((a % b) + b) % b
    match arithmetic::remainder_values(&args[0], &args[1]) {
        Ok(rem) => {
            // Add divisor to remainder
            match arithmetic::add_values(&rem, &args[1]) {
                Ok(sum) => match arithmetic::remainder_values(&sum, &args[1]) {
                    Ok(v) => (SIG_OK, v),
                    Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
                },
                Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
            }
        }
        Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
    }
}

// ── Comparison ──────────────────────────────────────────────────────

pub(super) fn prim_eq(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if *a == *b {
        return (SIG_OK, Value::TRUE);
    }
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            return (SIG_OK, Value::bool(x == y));
        }
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            return (SIG_OK, Value::bool(x == y));
        }
    }
    (SIG_OK, Value::FALSE)
}

pub(super) fn prim_ne(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (_, eq_result) = prim_eq(ctx, args);
    if eq_result == Value::TRUE {
        (SIG_OK, Value::FALSE)
    } else {
        (SIG_OK, Value::TRUE)
    }
}

pub(super) fn prim_lt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x < y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x < y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_lt()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_lt()));
    }
    type_err2("%lt", "number, string, or keyword", a, b, ctx)
}

pub(super) fn prim_gt(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x > y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x > y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_gt()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_gt()));
    }
    type_err2("%gt", "number, string, or keyword", a, b, ctx)
}

pub(super) fn prim_le(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x <= y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x <= y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_le()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_le()));
    }
    type_err2("%le", "number, string, or keyword", a, b, ctx)
}

pub(super) fn prim_ge(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = &args[0];
    let b = &args[1];
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return (SIG_OK, Value::bool(x >= y));
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        return (SIG_OK, Value::bool(x >= y));
    }
    if let Some(ord) = a.compare_str(b) {
        return (SIG_OK, Value::bool(ord.is_ge()));
    }
    if let Some(ord) = a.compare_keyword(b) {
        return (SIG_OK, Value::bool(ord.is_ge()));
    }
    type_err2("%ge", "number, string, or keyword", a, b, ctx)
}

// ── Logical ─────────────────────────────────────────────────────────

pub(super) fn prim_not(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(!args[0].is_truthy()))
}

// ── Conversion ──────────────────────────────────────────────────────

pub(super) fn prim_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::int(f as i64));
    }
    if args[0].as_int().is_some() {
        return (SIG_OK, args[0]);
    }
    type_err("%int", "number", &args[0], ctx)
}

pub(super) fn prim_float(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::float(n as f64));
    }
    if args[0].as_float().is_some() {
        return (SIG_OK, args[0]);
    }
    type_err("%float", "number", &args[0], ctx)
}

// ── Data ────────────────────────────────────────────────────────────

pub(super) fn prim_pair(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.pair(args[0], args[1]))
}

pub(super) fn prim_first(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(p) = args[0].as_pair() {
        return (SIG_OK, p.first);
    }
    type_err("%first", "pair", &args[0], ctx)
}

pub(super) fn prim_rest(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(p) = args[0].as_pair() {
        return (SIG_OK, p.rest);
    }
    type_err("%rest", "pair", &args[0], ctx)
}

// ── Bitwise ─────────────────────────────────────────────────────────

pub(super) fn prim_bit_and(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let a = args[0].as_int().ok_or(()).map_err(|_| ());
    let b = args[1].as_int().ok_or(()).map_err(|_| ());
    match (a, b) {
        (Ok(x), Ok(y)) => (SIG_OK, Value::int(x & y)),
        _ => type_err2("%bit-and", "integer", &args[0], &args[1], ctx),
    }
}

pub(super) fn prim_bit_or(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(x), Some(y)) => (SIG_OK, Value::int(x | y)),
        _ => type_err2("%bit-or", "integer", &args[0], &args[1], ctx),
    }
}

pub(super) fn prim_bit_xor(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(x), Some(y)) => (SIG_OK, Value::int(x ^ y)),
        _ => type_err2("%bit-xor", "integer", &args[0], &args[1], ctx),
    }
}

pub(super) fn prim_bit_not(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(!n)),
        None => type_err("%bit-not", "integer", &args[0], ctx),
    }
}

pub(super) fn prim_shl(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            let shift = b.clamp(0, 63) as u32;
            (SIG_OK, Value::int(a << shift))
        }
        _ => type_err2("%shl", "integer", &args[0], &args[1], ctx),
    }
}

pub(super) fn prim_shr(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match (args[0].as_int(), args[1].as_int()) {
        (Some(a), Some(b)) => {
            let shift = b.clamp(0, 63) as u32;
            (SIG_OK, Value::int(a >> shift))
        }
        _ => type_err2("%shr", "integer", &args[0], &args[1], ctx),
    }
}

// ── Type predicates ─────────────────────────────────────────────────
