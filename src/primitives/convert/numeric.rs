use super::*;

/// Numeric-only integer conversion. Accepts int (identity) or float (truncation).
/// String/keyword parsing is handled by `parse-int`.
pub(crate) fn prim_to_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::int(n));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::int(f as i64));
    }
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("integer: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Parse a string or keyword to integer, with optional radix (2–36).
pub(crate) fn prim_parse_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let radix: Option<u32> = if args.len() == 2 {
        match args[1].as_int() {
            Some(r) if (2..=36).contains(&r) => Some(r as u32),
            Some(r) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("parse-int: radix must be 2-36, got {}", r),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "parse-int: radix must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                );
            }
        }
    } else {
        None
    };

    if let Some(result) = args[0].with_string(|s| parse_int(ctx, s, radix)) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_int(ctx, &name, radix);
    }
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "parse-int: expected string or keyword, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn parse_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    s: &str,
    radix: Option<u32>,
) -> (SignalBits, Value) {
    let radix = radix.unwrap_or(10);
    match i64::from_str_radix(s, radix) {
        Ok(n) => (SIG_OK, Value::int(n)),
        Err(_) => crate::rich_error!(
            ctx,
            "parse-error",
            format!("integer: cannot parse \"{}\" as base-{} integer", s, radix),
            input = ctx.string(s),
        ),
    }
}

/// Numeric-only float conversion. Accepts int (→ f64) or float (identity).
/// String/keyword parsing is handled by `parse-float`.
pub(crate) fn prim_to_float(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::float(n as f64));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::float(f));
    }
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("float: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Parse a string or keyword to float.
pub(crate) fn prim_parse_float(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(result) = args[0].with_string(|s| parse_float(ctx, s)) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_float(ctx, &name);
    }
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "parse-float: expected string or keyword, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn parse_float(ctx: &mut crate::primitives::ctx::NativeCtx<'_>, s: &str) -> (SignalBits, Value) {
    match s.parse::<f64>() {
        Ok(f) => (SIG_OK, Value::float(f)),
        Err(_) => crate::rich_error!(
            ctx,
            "parse-error",
            format!("float: cannot parse \"{}\" as float", s),
            input = ctx.string(s),
        ),
    }
}

/// Convert integer to string with optional radix (2–36).
///
/// 1 arg: `(number->string n)` — decimal string for int or float.
/// 2 args: `(number->string n radix)` — convert integer `n` to string in the
///   given base. Float with radix → type-error.
pub(crate) fn prim_number_to_string(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.len() == 1 {
        // 1-arg: integer or float, decimal
        if let Some(n) = args[0].as_int() {
            return (SIG_OK, ctx.string(n.to_string()));
        }
        if let Some(f) = args[0].as_float() {
            let s = if f.fract() == 0.0 && f.is_finite() {
                format!("{:.1}", f)
            } else {
                f.to_string()
            };
            return (SIG_OK, ctx.string(s));
        }
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "number->string: expected number, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }

    // 2-arg: integer n + radix
    // Float with radix is an error.
    if args[0].as_float().is_some() && args[0].as_int().is_none() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "number->string: radix conversion requires integer, got float".to_string(),
            ),
        );
    }
    let n = match args[0].as_int() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "number->string: expected number, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let radix = match args[1].as_int() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "number->string: radix must be integer, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    if !(2..=36).contains(&radix) {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                format!("number->string: radix must be 2-36, got {}", radix),
            ),
        );
    }
    (SIG_OK, ctx.string(int_to_radix_string(n, radix as u32)))
}

/// Convert an i64 to a string in the given base (2–36), lowercase.
/// Sign is preserved: negative values produce a leading '-'.
fn int_to_radix_string(n: i64, radix: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let negative = n < 0;
    // Use u64 to avoid overflow on i64::MIN
    let mut value = if negative {
        (n as i128).unsigned_abs() as u64
    } else {
        n as u64
    };
    let mut buf = Vec::new();
    while value > 0 {
        buf.push(DIGITS[(value % radix as u64) as usize]);
        value /= radix as u64;
    }
    if negative {
        buf.push(b'-');
    }
    buf.reverse();
    String::from_utf8(buf).expect("digit chars are valid UTF-8")
}
