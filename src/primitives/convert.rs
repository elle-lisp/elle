//! Type conversion primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Convert to integer. Accepts int, float, string, or keyword.
pub(crate) fn prim_to_int(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("integer: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::int(n));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::int(f as i64));
    }
    if let Some(result) = args[0].with_string(parse_int) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_int(name);
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "integer: expected integer, float, string, or keyword, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn parse_int(s: &str) -> (SignalBits, Value) {
    match s.parse::<i64>() {
        Ok(n) => (SIG_OK, Value::int(n)),
        Err(_) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("integer: cannot parse \"{}\" as integer", s),
            ),
        ),
    }
}

/// Convert to float. Accepts int, float, string, or keyword.
pub(crate) fn prim_to_float(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("float: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(n) = args[0].as_int() {
        return (SIG_OK, Value::float(n as f64));
    }
    if let Some(f) = args[0].as_float() {
        return (SIG_OK, Value::float(f));
    }
    if let Some(result) = args[0].with_string(parse_float) {
        return result;
    }
    if let Some(name) = args[0].as_keyword_name() {
        return parse_float(name);
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "float: expected integer, float, string, or keyword, got {}",
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
            error_val("error", format!("float: cannot parse \"{}\" as float", s)),
        ),
    }
}

/// Convert to string (variadic: 0 args → "", 1 arg → convert, N args → concatenate)
pub(crate) fn prim_to_string(args: &[Value]) -> (SignalBits, Value) {
    match args.len() {
        0 => (SIG_OK, Value::string("")),
        1 => prim_to_string_single(args[0]),
        _ => {
            let mut result = String::new();
            for arg in args {
                let (sig, val) = prim_to_string_single(*arg);
                if sig != SIG_OK {
                    return (sig, val);
                }
                if let Some(s) = val.with_string(|s| s.to_string()) {
                    result.push_str(&s);
                } else {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-string: internal conversion failure".to_string(),
                        ),
                    );
                }
            }
            (SIG_OK, Value::string(result))
        }
    }
}

/// Single-value string conversion (original behavior).
fn prim_to_string_single(val: Value) -> (SignalBits, Value) {
    // Handle immediate types
    if val.is_string() {
        return (SIG_OK, val);
    }

    // @string: return as-is (preserves mutability)
    if val.as_string_mut().is_some() {
        return (SIG_OK, val);
    }

    // bytes (immutable): UTF-8 decode to immutable string
    if let Some(b) = val.as_bytes() {
        return match std::str::from_utf8(b) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    // @bytes (mutable): UTF-8 decode to mutable @string
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return match std::str::from_utf8(&borrowed) {
            Ok(_) => (SIG_OK, Value::string_mut(borrowed.clone())),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    if let Some(n) = val.as_int() {
        return (SIG_OK, Value::string(n.to_string()));
    }

    if let Some(f) = val.as_float() {
        return (SIG_OK, Value::string(f.to_string()));
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
                    "error",
                    format!("to-string: symbol ID {} not found in symbol table", sym_id),
                ),
            ),
        };
    }

    if let Some(name) = val.as_keyword_name() {
        return (SIG_OK, Value::string(name));
    }

    // Handle heap types (Cons, Array, etc.)
    if let Some(_cons) = val.as_cons() {
        let mut items = Vec::new();
        let mut current = val;
        loop {
            if current.is_nil() || current.is_empty_list() {
                break;
            }
            if let Some(c) = current.as_cons() {
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
                        "error",
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
                        "error",
                        "to-string: failed to convert array item".to_string(),
                    ),
                );
            }
        }

        let vec_str = format!("[{}]", formatted_items.join(", "));
        return (SIG_OK, Value::string(vec_str));
    }

    // For other types, use a reasonable debug representation
    (SIG_OK, Value::string(format!("{:?}", val)))
}

/// Declarative primitive definitions for conversion module.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "integer",
        func: prim_to_int,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Convert value to integer (48-bit signed). Accepts int, float, string, or keyword.",
        params: &["x"],
        category: "conversion",
        example: "(integer 3.7) #=> 3\n(integer \"42\") #=> 42",
        aliases: &["int"],
    },
    PrimitiveDef {
        name: "float",
        func: prim_to_float,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Convert value to float. Accepts int, float, string, or keyword.",
        params: &["x"],
        category: "conversion",
        example: "(float 42) #=> 42.0\n(float \"3.14\") #=> 3.14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string",
        func: prim_to_string,
        signal: Signal::inert(),
        arity: Arity::AtLeast(0),
        doc: "Convert values to string. Multiple arguments are concatenated.",
        params: &["values"],
        category: "conversion",
        example: "(string \"count: \" 42) #=> \"count: 42\"",
        aliases: &["any->string", "number->string", "symbol->string"],
    },
];
