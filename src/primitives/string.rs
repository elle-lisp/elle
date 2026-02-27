//! String manipulation primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Extract text content from a string or buffer value.
/// Returns (text, is_buffer). For buffers, validates UTF-8.
fn as_text(val: &Value, prim_name: &str) -> Result<(String, bool), (SignalBits, Value)> {
    if let Some(s) = val.as_string() {
        Ok((s.to_string(), false))
    } else if let Some(buf_ref) = val.as_buffer() {
        let borrowed = buf_ref.borrow();
        match String::from_utf8(borrowed.clone()) {
            Ok(s) => Ok((s, true)),
            Err(e) => Err((
                SIG_ERROR,
                error_val(
                    "error",
                    format!("{}: buffer contains invalid UTF-8: {}", prim_name, e),
                ),
            )),
        }
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected string or buffer, got {}",
                    prim_name,
                    val.type_name()
                ),
            ),
        ))
    }
}

/// Convert string or buffer to uppercase
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
    let (s, is_buffer) = match as_text(&args[0], "string-upcase") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let upper = s.to_uppercase();
    if is_buffer {
        (SIG_OK, Value::buffer(upper.into_bytes()))
    } else {
        (SIG_OK, Value::string(upper))
    }
}

/// Convert string or buffer to lowercase
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
    let (s, is_buffer) = match as_text(&args[0], "string-downcase") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let lower = s.to_lowercase();
    if is_buffer {
        (SIG_OK, Value::buffer(lower.into_bytes()))
    } else {
        (SIG_OK, Value::string(lower))
    }
}

/// Get a substring from a string or buffer
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

    let (s, _is_buffer) = match as_text(&args[0], "substring") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let s = s.as_str();

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
        return (SIG_OK, Value::NIL);
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

/// Find the index of a character in a string or buffer
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

    let (haystack, _is_buffer) = match as_text(&args[0], "string-index") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let needle = match args[1].as_string() {
        Some(s) => {
            let chars: Vec<char> = s.chars().collect();
            if chars.len() != 1 {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        "string-index: requires a single character as second argument".to_string(),
                    ),
                );
            }
            chars[0]
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

/// Get a character at an index from a string or buffer
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

    let (s, _is_buffer) = match as_text(&args[0], "char-at") {
        Ok(v) => v,
        Err(e) => return e,
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

/// Split string or buffer on delimiter
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

    let (s, _is_buffer) = match as_text(&args[0], "string-split") {
        Ok(v) => v,
        Err(e) => return e,
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

/// Replace all occurrences of old with new in a string or buffer
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

    let (s, is_buffer) = match as_text(&args[0], "string-replace") {
        Ok(v) => v,
        Err(e) => return e,
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

    let replaced = s.replace(old, new);
    if is_buffer {
        (SIG_OK, Value::buffer(replaced.into_bytes()))
    } else {
        (SIG_OK, Value::string(replaced))
    }
}

/// Trim leading and trailing whitespace from a string or buffer
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

    let (s, is_buffer) = match as_text(&args[0], "string-trim") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let trimmed = s.trim().to_string();
    if is_buffer {
        (SIG_OK, Value::buffer(trimmed.into_bytes()))
    } else {
        (SIG_OK, Value::string(trimmed))
    }
}

/// Check if string or buffer contains substring
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

    let (haystack, _is_buffer) = match as_text(&args[0], "string-contains?") {
        Ok(v) => v,
        Err(e) => return e,
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

/// Check if string or buffer starts with prefix
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

    let (s, _is_buffer) = match as_text(&args[0], "string-starts-with?") {
        Ok(v) => v,
        Err(e) => return e,
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

/// Check if string or buffer ends with suffix
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

    let (s, _is_buffer) = match as_text(&args[0], "string-ends-with?") {
        Ok(v) => v,
        Err(e) => return e,
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

/// Declarative primitive definitions for string module.
pub const PRIMITIVES: &[PrimitiveDef] = &[
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
        example: "(string/contains? \"hello\" \"ell\") ;=> true",
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
        example: "(string/starts-with? \"hello\" \"he\") ;=> true",
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
        example: "(string/ends-with? \"hello\" \"lo\") ;=> true",
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
];
