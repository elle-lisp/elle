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
