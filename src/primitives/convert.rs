//! Type conversion primitives
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, error_val_extra, Value};

/// Numeric-only integer conversion. Accepts int (identity) or float (truncation).
/// String/keyword parsing is handled by `parse-int`.
pub(crate) fn prim_to_int(args: &[Value]) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::int(n));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::int(f as i64));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("integer: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Parse a string or keyword to integer, with optional radix (2–36).
pub(crate) fn prim_parse_int(args: &[Value]) -> (SignalBits, Value) {
    let radix: Option<u32> = if args.len() == 2 {
        match args[1].as_int() {
            Some(r) if (2..=36).contains(&r) => Some(r as u32),
            Some(r) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("parse-int: radix must be 2-36, got {}", r),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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

    if let Some(result) = args[0].with_string(|s| parse_int(s, radix)) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_int(&name, radix);
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "parse-int: expected string or keyword, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn parse_int(s: &str, radix: Option<u32>) -> (SignalBits, Value) {
    let radix = radix.unwrap_or(10);
    match i64::from_str_radix(s, radix) {
        Ok(n) => (SIG_OK, Value::int(n)),
        Err(_) => (
            SIG_ERROR,
            error_val_extra(
                "parse-error",
                format!("integer: cannot parse \"{}\" as base-{} integer", s, radix),
                &[("input", Value::string(s))],
            ),
        ),
    }
}

/// Numeric-only float conversion. Accepts int (→ f64) or float (identity).
/// String/keyword parsing is handled by `parse-float`.
pub(crate) fn prim_to_float(args: &[Value]) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::float(n as f64));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::float(f));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("float: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Parse a string or keyword to float.
pub(crate) fn prim_parse_float(args: &[Value]) -> (SignalBits, Value) {
    if let Some(result) = args[0].with_string(parse_float) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_float(&name);
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "parse-float: expected string or keyword, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn parse_float(s: &str) -> (SignalBits, Value) {
    match s.parse::<f64>() {
        Ok(f) => (SIG_OK, Value::float(f)),
        Err(_) => (
            SIG_ERROR,
            error_val_extra(
                "parse-error",
                format!("float: cannot parse \"{}\" as float", s),
                &[("input", Value::string(s))],
            ),
        ),
    }
}

/// Convert integer to string with optional radix (2–36).
///
/// 1 arg: `(number->string n)` — decimal string for int or float.
/// 2 args: `(number->string n radix)` — convert integer `n` to string in the
///   given base. Float with radix → type-error.
pub(crate) fn prim_number_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() == 1 {
        // 1-arg: integer or float, decimal
        if let Some(n) = args[0].as_int() {
            return (SIG_OK, Value::string(n.to_string()));
        }
        if let Some(f) = args[0].as_float() {
            let s = if f.fract() == 0.0 && f.is_finite() {
                format!("{:.1}", f)
            } else {
                f.to_string()
            };
            return (SIG_OK, Value::string(s));
        }
        return (
            SIG_ERROR,
            error_val(
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
            error_val(
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
                error_val(
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
                error_val(
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
            error_val(
                "argument-error",
                format!("number->string: radix must be 2-36, got {}", radix),
            ),
        );
    }
    (SIG_OK, Value::string(int_to_radix_string(n, radix as u32)))
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

/// Convert to string (variadic: 0 args → "", 1 arg → convert, N args → concatenate)
pub(crate) fn prim_to_string(args: &[Value]) -> (SignalBits, Value) {
    match args.len() {
        0 => (SIG_OK, Value::string("")),
        1 => prim_to_string_single(args[0]),
        _ => {
            // Multi-arg: format directly into a Rust String to avoid
            // allocating slab-backed intermediate strings per argument.
            let mut result = String::new();
            for arg in args {
                if let Err((sig, val)) = write_value_to_string(*arg, &mut result) {
                    return (sig, val);
                }
            }
            (SIG_OK, Value::string(result))
        }
    }
}

/// Append a value's string representation directly to a Rust String,
/// avoiding slab allocation for intermediates.
fn write_value_to_string(val: Value, out: &mut String) -> Result<(), (SignalBits, Value)> {
    use std::fmt::Write;

    if val.is_string() {
        val.with_string(|s| out.push_str(s));
        return Ok(());
    }
    if let Some(ms) = val.as_string_mut() {
        let borrowed = ms.borrow();
        match std::str::from_utf8(&borrowed) {
            Ok(s) => out.push_str(s),
            Err(e) => {
                return Err((
                    SIG_ERROR,
                    error_val("encoding-error", format!("string: invalid UTF-8: {}", e)),
                ))
            }
        }
        return Ok(());
    }
    if let Some(n) = val.as_int() {
        let _ = write!(out, "{}", n);
        return Ok(());
    }
    if let Some(f) = val.as_float() {
        if f.is_infinite() {
            out.push_str(if f.is_sign_positive() { "inf" } else { "-inf" });
        } else if f.is_nan() {
            out.push_str("NaN");
        } else if f.fract() == 0.0 {
            let _ = write!(out, "{:.1}", f);
        } else {
            let _ = write!(out, "{}", f);
        }
        return Ok(());
    }
    if let Some(b) = val.as_bool() {
        out.push_str(if b { "true" } else { "false" });
        return Ok(());
    }
    if val.is_nil() {
        out.push_str("nil");
        return Ok(());
    }
    if let Some(name) = val.as_keyword_name() {
        out.push_str(&name);
        return Ok(());
    }
    if let Some(sym_id) = val.as_symbol() {
        match crate::context::resolve_symbol_name(sym_id) {
            Some(name) => out.push_str(&name),
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        format!("to-string: symbol ID {} not found in symbol table", sym_id),
                    ),
                ))
            }
        }
        return Ok(());
    }
    // For compound/heap types, fall back to prim_to_string_single
    // (these are rare in hot concat paths).
    let (sig, string_val) = prim_to_string_single(val);
    if sig != SIG_OK {
        return Err((sig, string_val));
    }
    if let Some(s) = string_val.with_string(|s| s.to_string()) {
        out.push_str(&s);
    } else {
        return Err((
            SIG_ERROR,
            error_val(
                "internal-error",
                "to-string: internal conversion failure".to_string(),
            ),
        ));
    }
    Ok(())
}

/// Single-value string conversion (original behavior).
fn prim_to_string_single(val: Value) -> (SignalBits, Value) {
    // Handle immediate types
    if val.is_string() {
        return (SIG_OK, val);
    }

    // @string: convert to immutable string
    if let Some(ms) = val.as_string_mut() {
        let borrowed = ms.borrow();
        return match std::str::from_utf8(&borrowed) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    // bytes (immutable): UTF-8 decode to immutable string
    if let Some(b) = val.as_bytes() {
        return match std::str::from_utf8(b) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    // @bytes (mutable): UTF-8 decode to immutable string
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return match std::str::from_utf8(&borrowed) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    if let Some(n) = val.as_int() {
        return (SIG_OK, Value::string(n.to_string()));
    }

    if let Some(f) = val.as_float() {
        let s = if f.is_infinite() {
            if f.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else if f.is_nan() {
            "NaN".to_string()
        } else if f.fract() == 0.0 {
            format!("{:.1}", f)
        } else {
            f.to_string()
        };
        return (SIG_OK, Value::string(s));
    }

    if let Some(b) = val.as_bool() {
        return (SIG_OK, Value::string(if b { "true" } else { "false" }));
    }

    if val.is_nil() {
        return (SIG_OK, Value::string("nil"));
    }

    if let Some(sym_id) = val.as_symbol() {
        return match crate::context::resolve_symbol_name(sym_id) {
            Some(name) => (SIG_OK, Value::string(name)),
            None => (
                SIG_ERROR,
                error_val(
                    "internal-error",
                    format!("to-string: symbol ID {} not found in symbol table", sym_id),
                ),
            ),
        };
    }

    if let Some(name) = val.as_keyword_name() {
        return (SIG_OK, Value::string(name));
    }

    // Handle heap types (Pair, Array, etc.)
    if let Some(_cons) = val.as_pair() {
        let mut items = Vec::new();
        let mut current = val;
        loop {
            if current.is_nil() || current.is_empty_list() {
                break;
            }
            if let Some(c) = current.as_pair() {
                items.push(c.first);
                current = c.rest;
            } else {
                items.push(current);
                break;
            }
        }

        let mut formatted_items = Vec::new();
        for v in items {
            let (sig, result) = prim_to_string_single(v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "to-string: failed to convert list item".to_string(),
                    ),
                );
            }
        }

        let list_str = format!("({})", formatted_items.join(" "));
        return (SIG_OK, Value::string(list_str));
    }

    if let Some(vec_ref) = val.as_array_mut() {
        let vec = vec_ref.borrow();
        let mut formatted_items = Vec::new();
        for v in vec.iter() {
            let (sig, result) = prim_to_string_single(*v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "to-string: failed to convert array item".to_string(),
                    ),
                );
            }
        }

        let vec_str = format!("@[{}]", formatted_items.join(" "));
        return (SIG_OK, Value::string(vec_str));
    }

    if let Some(elems) = val.as_array() {
        let mut formatted_items = Vec::new();
        for v in elems.iter() {
            let (sig, result) = prim_to_string_single(*v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "to-string: failed to convert array item".to_string(),
                    ),
                );
            }
        }

        let vec_str = format!("[{}]", formatted_items.join(" "));
        return (SIG_OK, Value::string(vec_str));
    }

    // For other types, use a reasonable debug representation
    (SIG_OK, Value::string(format!("{:?}", val)))
}

/// Declarative primitive definitions for conversion module.
primitive! {
    "integer" => prim_to_int {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert number to integer (i64). Accepts int (identity) or float (truncation). Use parse-int for string→int.",
        params: &["x"],
        category: "conversion",
        example: "(integer 3.7) #=> 3\n(integer 42) #=> 42",
        aliases: &["int"],
    }
    "float" => prim_to_float {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert number to float. Accepts int (→ f64) or float (identity). Use parse-float for string→float.",
        params: &["x"],
        category: "conversion",
        example: "(float 42) #=> 42.0\n(float 3.14) #=> 3.14",
    }
    "parse-int" => prim_parse_int {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Parse string or keyword to integer. Optional radix (2–36) for base conversion.",
        params: &["s", "radix?"],
        category: "conversion",
        example: "(parse-int \"42\") #=> 42\n(parse-int \"ff\" 16) #=> 255\n(parse-int \"1010\" 2) #=> 10",
    }
    "parse-float" => prim_parse_float {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse string or keyword to float.",
        params: &["s"],
        category: "conversion",
        example: "(parse-float \"3.14\") #=> 3.14",
    }
    "string" => prim_to_string {
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Convert values to string. Multiple arguments are concatenated.",
        params: &["values"],
        category: "conversion",
        example: "(string \"count: \" 42) #=> \"count: 42\"",
        aliases: &["any->string", "symbol->string"],
    }
    "number->string" => prim_number_to_string {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Convert a number to string. With an optional radix (2–36), converts an integer to the given base (lowercase, no prefix).",
        params: &["n", "radix?"],
        category: "conversion",
        example: "(number->string 42) #=> \"42\"\n(number->string 255 16) #=> \"ff\"\n(number->string -255 16) #=> \"-ff\"",
    }
}
