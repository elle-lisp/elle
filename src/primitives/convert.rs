//! Type conversion primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Convert to integer
pub fn prim_to_int(args: &[Value]) -> (SignalBits, Value) {
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
            None => match args[0].as_string() {
                Some(s) => match s.parse::<i64>() {
                    Ok(n) => (SIG_OK, Value::int(n)),
                    Err(_) => (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-int: cannot parse string as integer".to_string(),
                        ),
                    ),
                },
                None => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "to-int: expected integer, float, or string, got {}",
                            args[0].type_name()
                        ),
                    ),
                ),
            },
        },
    }
}

/// Convert to float
pub fn prim_to_float(args: &[Value]) -> (SignalBits, Value) {
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
            None => match args[0].as_string() {
                Some(s) => match s.parse::<f64>() {
                    Ok(f) => (SIG_OK, Value::float(f)),
                    Err(_) => (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-float: cannot parse string as float".to_string(),
                        ),
                    ),
                },
                None => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "to-float: expected integer, float, or string, got {}",
                            args[0].type_name()
                        ),
                    ),
                ),
            },
        },
    }
}

/// Convert to string
pub fn prim_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("to-string: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let val = args[0];

    // Handle immediate types
    if let Some(s) = val.as_string() {
        return (SIG_OK, Value::string(s));
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
        return (SIG_OK, Value::string(format!(":{}", name)));
    }

    // Handle heap types (Cons, Array, etc.)
    if let Some(_cons) = val.as_cons() {
        // Format as list "(1 2 3)"
        // Lists are EMPTY_LIST-terminated, not NIL-terminated.
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
                // Improper list - add the tail
                items.push(current);
                break;
            }
        }

        let mut formatted_items = Vec::new();
        for v in items {
            let (sig, result) = prim_to_string(&[v]);
            if sig != SIG_OK {
                return (sig, result);
            }
            match result.as_string() {
                Some(s) => formatted_items.push(s.to_string()),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-string: failed to convert list item".to_string(),
                        ),
                    )
                }
            }
        }

        let list_str = format!("({})", formatted_items.join(" "));
        return (SIG_OK, Value::string(list_str));
    }

    if let Some(vec_ref) = val.as_array() {
        // Format as "[1, 2, 3]"
        let vec = vec_ref.borrow();
        let mut formatted_items = Vec::new();
        for v in vec.iter() {
            let (sig, result) = prim_to_string(&[*v]);
            if sig != SIG_OK {
                return (sig, result);
            }
            match result.as_string() {
                Some(s) => formatted_items.push(s.to_string()),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            "to-string: failed to convert array item".to_string(),
                        ),
                    )
                }
            }
        }

        let vec_str = format!("[{}]", formatted_items.join(", "));
        return (SIG_OK, Value::string(vec_str));
    }

    // For other types, use a reasonable debug representation
    (SIG_OK, Value::string(format!("{:?}", val)))
}

/// Convert number to string
pub fn prim_number_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("number->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::string(n.to_string())),
        None => match args[0].as_float() {
            Some(f) => (SIG_OK, Value::string(f.to_string())),
            None => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "number->string: expected number, got {}",
                        args[0].type_name()
                    ),
                ),
            ),
        },
    }
}

// ============ SCHEME-STYLE CONVERSION ALIASES ============

/// Convert string to integer (Scheme-style name)
/// `(string->int str)`
pub fn prim_string_to_int(args: &[Value]) -> (SignalBits, Value) {
    prim_to_int(args)
}

/// Convert string to float (Scheme-style name)
/// `(string->float str)`
pub fn prim_string_to_float(args: &[Value]) -> (SignalBits, Value) {
    prim_to_float(args)
}

/// Convert any value to string (Scheme-style name)
/// `(any->string val)`
pub fn prim_any_to_string(args: &[Value]) -> (SignalBits, Value) {
    prim_to_string(args)
}

/// Convert keyword to string (without colon prefix)
/// `(keyword->string kw)` → `"name"`
pub fn prim_keyword_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keyword->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_keyword_name() {
        Some(name) => (SIG_OK, Value::string(name)),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "keyword->string: expected keyword, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// `(symbol->string sym)` → `"name"`
pub fn prim_symbol_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("symbol->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_symbol() {
        Some(id) => match crate::context::resolve_symbol_name(id) {
            Some(name) => (SIG_OK, Value::string(name)),
            None => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("Symbol ID {} not found in symbol table", id),
                ),
            ),
        },
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "symbol->string: expected symbol, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// Declarative primitive definitions for conversion module.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "number->string",
        func: prim_number_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert number to string.",
        params: &["n"],
        category: "conversion",
        example: "(number->string 42) ;=> \"42\"",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->integer",
        func: prim_string_to_int,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Parse string as integer.",
        params: &["s"],
        category: "conversion",
        example: "(string->integer \"42\") ;=> 42",
        aliases: &["string->int"],
    },
    PrimitiveDef {
        name: "string->float",
        func: prim_string_to_float,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Parse string as floating-point number.",
        params: &["s"],
        category: "conversion",
        example: "(string->float \"3.14\") ;=> 3.14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "any->string",
        func: prim_any_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert any value to its string representation.",
        params: &["x"],
        category: "conversion",
        example: "(any->string 42) ;=> \"42\"\n(any->string true) ;=> \"true\"",
        aliases: &[],
    },
    PrimitiveDef {
        name: "symbol->string",
        func: prim_symbol_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert symbol to string (without quote).",
        params: &["sym"],
        category: "conversion",
        example: "(symbol->string 'foo) ;=> \"foo\"",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keyword->string",
        func: prim_keyword_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert keyword to string (without colon prefix).",
        params: &["kw"],
        category: "conversion",
        example: "(keyword->string :foo) ;=> \"foo\"",
        aliases: &[],
    },
    PrimitiveDef {
        name: "integer",
        func: prim_to_int,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert value to integer. Accepts int, float, or string.",
        params: &["x"],
        category: "conversion",
        example: "(integer 3.7) ;=> 3\n(integer \"42\") ;=> 42",
        aliases: &["int"],
    },
    PrimitiveDef {
        name: "float",
        func: prim_to_float,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert value to float. Accepts int, float, or string.",
        params: &["x"],
        category: "conversion",
        example: "(float 42) ;=> 42.0\n(float \"3.14\") ;=> 3.14",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string",
        func: prim_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert value to string representation.",
        params: &["x"],
        category: "conversion",
        example: "(string 42) ;=> \"42\"\n(string (list 1 2)) ;=> \"(1 2)\"",
        aliases: &[],
    },
];
