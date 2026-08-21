//! Formatting a single value: type dispatch, then width/alignment padding.
use crate::primitives::ctx::NativeCtx;
use crate::primitives::formatspec::{
    parse_format_spec, spec_type_char, Align, FormatSpec, FormatType,
};
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::Value;

/// Format a single value according to a parsed format spec.
pub(super) fn format_value(
    value: &Value,
    spec_str: &str,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    let mut spec = parse_format_spec(spec_str, ctx)?;

    // Resolve default alignment based on value type:
    // numbers default to right-align, everything else to left-align.
    if spec.align == Align::Default {
        let is_numeric = value.as_int().is_some() || value.as_float().is_some();
        spec.align = if is_numeric {
            Align::Right
        } else {
            Align::Left
        };
    }

    // Get the raw formatted string (before width/align)
    let raw = format_raw(value, &spec, ctx)?;

    // Apply width and alignment
    apply_width_align(&raw, &spec)
}

/// Format the value's content without width/alignment padding.
fn format_raw(
    value: &Value,
    spec: &FormatSpec,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    // Integer formatting
    if let Some(n) = value.as_int() {
        return format_int(n, spec, ctx);
    }

    // Float formatting
    if let Some(f) = value.as_float() {
        return format_float(f, spec, ctx);
    }

    // String formatting
    if value.is_string() {
        return value
            .with_string(|s| format_string(s, spec, ctx))
            .unwrap_or_else(|| Ok(String::new()));
    }

    // For all other types: only None or StringType specs are valid
    match spec.ty {
        FormatType::None | FormatType::StringType => {
            let mut s = String::new();
            use std::fmt::Write;
            let _ = write!(s, "{}", value);
            if let Some(prec) = spec.precision {
                let truncated: String = s.chars().take(prec).collect();
                return Ok(truncated);
            }
            Ok(s)
        }
        _ => Err((
            SIG_ERROR,
            ctx.error(
                "format-error",
                format!(
                    "string/format: cannot format {} with spec '{}'",
                    value.type_name(),
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_int(
    n: i64,
    spec: &FormatSpec,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::Decimal => Ok(format!("{}", n)),
        FormatType::Hex => Ok(format!("{:x}", n)),
        FormatType::HexUpper => Ok(format!("{:X}", n)),
        FormatType::Octal => Ok(format!("{:o}", n)),
        FormatType::Binary => Ok(format!("{:b}", n)),
        FormatType::Float => {
            let f = n as f64;
            match spec.precision {
                Some(prec) => Ok(format!("{:.prec$}", f, prec = prec)),
                None => Ok(format!("{:.1}", f)),
            }
        }
        FormatType::Scientific => {
            let f = n as f64;
            match spec.precision {
                Some(prec) => Ok(format!("{:.prec$e}", f, prec = prec)),
                None => Ok(format!("{:e}", f)),
            }
        }
        _ => Err((
            SIG_ERROR,
            ctx.error(
                "format-error",
                format!(
                    "string/format: cannot format integer with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_float(
    f: f64,
    spec: &FormatSpec,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::Float => match spec.precision {
            Some(prec) => Ok(format!("{:.prec$}", f, prec = prec)),
            None => Ok(format!("{}", f)),
        },
        FormatType::Scientific => match spec.precision {
            Some(prec) => Ok(format!("{:.prec$e}", f, prec = prec)),
            None => Ok(format!("{:e}", f)),
        },
        FormatType::Decimal => Ok(format!("{}", f as i64)),
        FormatType::Hex => Ok(format!("{:x}", f as i64)),
        FormatType::HexUpper => Ok(format!("{:X}", f as i64)),
        FormatType::Octal => Ok(format!("{:o}", f as i64)),
        FormatType::Binary => Ok(format!("{:b}", f as i64)),
        _ => Err((
            SIG_ERROR,
            ctx.error(
                "format-error",
                format!(
                    "string/format: cannot format float with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn format_string(
    s: &str,
    spec: &FormatSpec,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    match spec.ty {
        FormatType::None | FormatType::StringType => {
            if let Some(prec) = spec.precision {
                Ok(s.chars().take(prec).collect())
            } else {
                Ok(s.to_string())
            }
        }
        _ => Err((
            SIG_ERROR,
            ctx.error(
                "format-error",
                format!(
                    "string/format: cannot format string with spec '{}'",
                    spec_type_char(spec.ty)
                ),
            ),
        )),
    }
}

fn apply_width_align(s: &str, spec: &FormatSpec) -> Result<String, (SignalBits, Value)> {
    let width = match spec.width {
        Some(w) => w,
        None => return Ok(s.to_string()),
    };

    let char_count = s.chars().count();
    if char_count >= width {
        return Ok(s.to_string());
    }

    let padding = width - char_count;
    let fill = spec.fill;

    // Align::Default is resolved in format_value before reaching here.
    let (left_pad, right_pad) = match spec.align {
        Align::Left => (0, padding),
        Align::Right => (padding, 0),
        Align::Center => {
            let left = padding / 2;
            let right = padding - left;
            (left, right)
        }
        Align::Default => unreachable!(),
    };

    let mut result = String::with_capacity(width);
    for _ in 0..left_pad {
        result.push(fill);
    }
    result.push_str(s);
    for _ in 0..right_pad {
        result.push(fill);
    }

    Ok(result)
}
