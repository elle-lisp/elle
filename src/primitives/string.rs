//! String manipulation primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Append multiple strings
pub fn prim_string_append(args: &[Value]) -> (SignalBits, Value) {
    let mut result = String::new();
    for arg in args {
        match arg.as_string() {
            Some(s) => result.push_str(s),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("string-append: expected string, got {}", arg.type_name()),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::string(result))
}

/// Convert string to uppercase
pub fn prim_string_upcase(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-upcase: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_string() {
        Some(s) => (SIG_OK, Value::string(s.to_uppercase())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-upcase: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// Convert string to lowercase
pub fn prim_string_downcase(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-downcase: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_string() {
        Some(s) => (SIG_OK, Value::string(s.to_lowercase())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-downcase: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// Get a substring
pub fn prim_substring(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("substring: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("substring: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let start = match args[1].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("substring: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let char_count = s.chars().count();
    let end = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) => n as usize,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("substring: expected integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    } else {
        char_count
    };

    if start > char_count || end > char_count || start > end {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "substring: index {} out of bounds (length {})",
                    start, char_count
                ),
            ),
        );
    }

    // Convert character indices to byte indices
    let byte_start = s
        .char_indices()
        .nth(start)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let byte_end = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    (SIG_OK, Value::string(&s[byte_start..byte_end]))
}

/// Find the index of a character
pub fn prim_string_index(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-index: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let haystack = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string-index: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let needle = match args[1].as_string() {
        Some(s) => {
            if s.chars().count() != 1 {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        "string-index: requires a single character as second argument".to_string(),
                    ),
                );
            }
            s.chars().next().unwrap()
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string-index: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    };

    match haystack.chars().position(|ch| ch == needle) {
        Some(pos) => (SIG_OK, Value::int(pos as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

/// Get a character at an index
pub fn prim_char_at(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("char-at: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("char-at: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let index = match args[1].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("char-at: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let char_count = s.chars().count();

    if index >= char_count {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "char-at: index {} out of bounds (length {})",
                    index, char_count
                ),
            ),
        );
    }

    match s.chars().nth(index) {
        Some(c) => (SIG_OK, Value::string(c.to_string())),
        None => (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "char-at: index {} out of bounds (length {})",
                    index, char_count
                ),
            ),
        ),
    }
}

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
        return match crate::ffi::primitives::context::resolve_symbol_name(sym_id) {
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
        let mut items = Vec::new();
        let mut current = val;
        loop {
            if current.is_nil() {
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

/// Split string on delimiter
pub fn prim_string_split(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-split: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string-split: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let delimiter = match args[1].as_string() {
        Some(d) => d,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string-split: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if delimiter.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "string-split: delimiter cannot be empty".to_string(),
            ),
        );
    }

    let parts: Vec<Value> = s.split(delimiter).map(Value::string).collect();

    (SIG_OK, crate::value::list(parts))
}

/// Replace all occurrences of old with new
pub fn prim_string_replace(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-replace: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-replace: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let old = match args[1].as_string() {
        Some(o) => o,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-replace: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    if old.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "string-replace: search string cannot be empty".to_string(),
            ),
        );
    }

    let new = match args[2].as_string() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-replace: expected string, got {}",
                        args[2].type_name()
                    ),
                ),
            )
        }
    };

    (SIG_OK, Value::string(s.replace(old, new)))
}

/// Trim leading and trailing whitespace
pub fn prim_string_trim(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-trim: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    match args[0].as_string() {
        Some(s) => (SIG_OK, Value::string(s.trim())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("string-trim: expected string, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// Check if string contains substring
pub fn prim_string_contains(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-contains?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let haystack = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-contains?: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let needle = match args[1].as_string() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-contains?: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    (
        SIG_OK,
        if haystack.contains(needle) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string starts with prefix
pub fn prim_string_starts_with(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "string-starts-with?: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-starts-with?: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let prefix = match args[1].as_string() {
        Some(p) => p,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-starts-with?: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    (
        SIG_OK,
        if s.starts_with(prefix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string ends with suffix
pub fn prim_string_ends_with(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "string-ends-with?: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }

    let s = match args[0].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-ends-with?: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let suffix = match args[1].as_string() {
        Some(suf) => suf,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "string-ends-with?: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    (
        SIG_OK,
        if s.ends_with(suffix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Join list of strings with separator
pub fn prim_string_join(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-join: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let list = &args[0];
    let separator = match args[1].as_string() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string-join: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let vec = match list.list_to_vec() {
        Ok(v) => v,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("string-join: {}", e)),
            )
        }
    };
    let mut strings = Vec::new();

    for val in vec {
        match val.as_string() {
            Some(s) => strings.push(s.to_string()),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("string-join: expected string, got {}", val.type_name()),
                    ),
                )
            }
        }
    }

    (SIG_OK, Value::string(strings.join(separator)))
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
        Some(id) => match crate::ffi::primitives::context::resolve_symbol_name(id) {
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

/// Declarative primitive definitions for string module.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "string/append",
        func: prim_string_append,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Concatenate all string arguments.",
        params: &["strs"],
        category: "string",
        example: "(string/append \"hello\" \" \" \"world\") ;=> \"hello world\"",
        aliases: &["string-append"],
    },
    PrimitiveDef {
        name: "string/upcase",
        func: prim_string_upcase,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert string to uppercase.",
        params: &["s"],
        category: "string",
        example: "(string/upcase \"hello\") ;=> \"HELLO\"",
        aliases: &["string-upcase"],
    },
    PrimitiveDef {
        name: "string/downcase",
        func: prim_string_downcase,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert string to lowercase.",
        params: &["s"],
        category: "string",
        example: "(string/downcase \"HELLO\") ;=> \"hello\"",
        aliases: &["string-downcase"],
    },
    PrimitiveDef {
        name: "string/slice",
        func: prim_substring,
        effect: Effect::none(),
        arity: Arity::Range(2, 3),
        doc: "Extract substring from start to end (exclusive). End defaults to string length.",
        params: &["s", "start", "end"],
        category: "string",
        example: "(string/slice \"hello\" 1 4) ;=> \"ell\"",
        aliases: &["substring"],
    },
    PrimitiveDef {
        name: "string/index",
        func: prim_string_index,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Find index of first occurrence of substring. Returns nil if not found.",
        params: &["s", "substr"],
        category: "string",
        example: "(string/index \"hello\" \"l\") ;=> 2",
        aliases: &["string-index"],
    },
    PrimitiveDef {
        name: "string/char-at",
        func: prim_char_at,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Get character at index as a single-character string.",
        params: &["s", "idx"],
        category: "string",
        example: "(string/char-at \"hello\" 1) ;=> \"e\"",
        aliases: &["char-at"],
    },
    PrimitiveDef {
        name: "string/split",
        func: prim_string_split,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Split string by delimiter, returning a list of substrings.",
        params: &["s", "delim"],
        category: "string",
        example: "(string/split \"a,b,c\" \",\") ;=> (\"a\" \"b\" \"c\")",
        aliases: &["string-split"],
    },
    PrimitiveDef {
        name: "string/replace",
        func: prim_string_replace,
        effect: Effect::none(),
        arity: Arity::Exact(3),
        doc: "Replace all occurrences of old substring with new.",
        params: &["s", "old", "new"],
        category: "string",
        example: "(string/replace \"hello\" \"l\" \"L\") ;=> \"heLLo\"",
        aliases: &["string-replace"],
    },
    PrimitiveDef {
        name: "string/trim",
        func: prim_string_trim,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Remove leading and trailing whitespace.",
        params: &["s"],
        category: "string",
        example: "(string/trim \"  hello  \") ;=> \"hello\"",
        aliases: &["string-trim"],
    },
    PrimitiveDef {
        name: "string/contains?",
        func: prim_string_contains,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Check if string contains substring.",
        params: &["s", "substr"],
        category: "string",
        example: "(string/contains? \"hello\" \"ell\") ;=> #t",
        aliases: &["string-contains?"],
    },
    PrimitiveDef {
        name: "string/starts-with?",
        func: prim_string_starts_with,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Check if string starts with prefix.",
        params: &["s", "prefix"],
        category: "string",
        example: "(string/starts-with? \"hello\" \"he\") ;=> #t",
        aliases: &["string-starts-with?"],
    },
    PrimitiveDef {
        name: "string/ends-with?",
        func: prim_string_ends_with,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Check if string ends with suffix.",
        params: &["s", "suffix"],
        category: "string",
        example: "(string/ends-with? \"hello\" \"lo\") ;=> #t",
        aliases: &["string-ends-with?"],
    },
    PrimitiveDef {
        name: "string/join",
        func: prim_string_join,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Join list of strings with separator.",
        params: &["lst", "sep"],
        category: "string",
        example: "(string/join (list \"a\" \"b\" \"c\") \",\") ;=> \"a,b,c\"",
        aliases: &["string-join"],
    },
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
        example: "(any->string 42) ;=> \"42\"\n(any->string #t) ;=> \"true\"",
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
