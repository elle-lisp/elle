//! Type conversion primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Convert to integer
pub(crate) fn prim_to_int(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("to-int: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::int(n)),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::int(f as i64)),
            None => {
                if let Some(result) = args[0].with_string(|s| match s.parse::<i64>() {
                    Ok(n) => (SIG_OK, Value::int(n)),
                    Err(_) => (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-int: cannot parse string as integer".to_string(),
                        ),
                    ),
                }) {
                    result
                } else {
                    (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "to-int: expected integer, float, or string, got {}",
                                args[0].type_name()
                            ),
                        ),
                    )
                }
            }
        },
    }
}

/// Convert to float
pub(crate) fn prim_to_float(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("to-float: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::float(n as f64)),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::float(f)),
            None => {
                if let Some(result) = args[0].with_string(|s| match s.parse::<f64>() {
                    Ok(f) => (SIG_OK, Value::float(f)),
                    Err(_) => (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-float: cannot parse string as float".to_string(),
                        ),
                    ),
                }) {
                    result
                } else {
                    (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "to-float: expected integer, float, or string, got {}",
                                args[0].type_name()
                            ),
                        ),
                    )
                }
            }
        },
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

// ============ SCHEME-STYLE CONVERSION ALIASES ============

/// Convert string to integer (Scheme-style name)
/// `(string->int str)`
pub(crate) fn prim_string_to_int(args: &[Value]) -> (SignalBits, Value) {
    prim_to_int(args)
}

/// Convert string to float (Scheme-style name)
/// `(string->float str)`
pub(crate) fn prim_string_to_float(args: &[Value]) -> (SignalBits, Value) {
    prim_to_float(args)
}

/// Declarative primitive definitions for conversion module.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "string->integer",
        func: prim_string_to_int,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Parse string as integer.",
        params: &["s"],
        category: "conversion",
        example: "(string->integer \"42\") #=> 42",
        aliases: &["string->int"],
    },
    PrimitiveDef {
        name: "string->float",
        func: prim_string_to_float,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Parse string as floating-point number.",
        params: &["s"],
        category: "conversion",
        example: "(string->float \"3.14\") #=> 3.14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "integer",
        func: prim_to_int,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert value to integer (48-bit signed). Accepts int, float, or string.",
        params: &["x"],
        category: "conversion",
        example: "(integer 3.7) #=> 3\n(integer \"42\") #=> 42",
        aliases: &["int"],
    },
    PrimitiveDef {
        name: "float",
        func: prim_to_float,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert value to float. Accepts int, float, or string.",
        params: &["x"],
        category: "conversion",
        example: "(float 42) #=> 42.0\n(float \"3.14\") #=> 3.14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string",
        func: prim_to_string,
        effect: Effect::inert(),
        arity: Arity::AtLeast(0),
        doc: "Convert values to string. Multiple arguments are concatenated.",
        params: &["values"],
        category: "conversion",
        example: "(string \"count: \" 42) #=> \"count: 42\"",
        aliases: &["any->string", "number->string", "symbol->string"],
    },
];
