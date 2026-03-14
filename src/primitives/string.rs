//! String manipulation primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Extract text content from a string or @string value.
/// Returns (text, is_@string). For @strings, validates UTF-8.
fn as_text(val: &Value, prim_name: &str) -> Result<(String, bool), (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_string()) {
        Ok((s, false))
    } else if let Some(buf_ref) = val.as_string_mut() {
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
pub(crate) fn prim_string_upcase(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::string_mut(upper.into_bytes()))
    } else {
        (SIG_OK, Value::string(upper))
    }
}

/// Convert string or buffer to lowercase
pub(crate) fn prim_string_downcase(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::string_mut(lower.into_bytes()))
    } else {
        (SIG_OK, Value::string(lower))
    }
}

/// Find the grapheme index of a substring, with optional start offset
pub(crate) fn prim_string_find(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string/find: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let (haystack, _is_buffer) = match as_text(&args[0], "string/find") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let needle = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("string/find: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let offset = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) if n >= 0 => n as usize,
            Some(_) => return (SIG_OK, Value::NIL),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "string/find: offset must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        }
    } else {
        0
    };

    let graphemes: Vec<&str> = haystack.graphemes(true).collect();

    if offset > graphemes.len() {
        return (SIG_OK, Value::NIL);
    }

    // Build the substring from offset onwards, then search
    let search_start_byte: usize = graphemes[..offset].iter().map(|g| g.len()).sum();
    match haystack[search_start_byte..].find(&needle) {
        Some(byte_pos) => {
            // Convert byte position back to grapheme index
            let abs_byte = search_start_byte + byte_pos;
            let mut byte_idx = 0;
            for (grapheme_idx, g) in graphemes.iter().enumerate() {
                if byte_idx == abs_byte {
                    return (SIG_OK, Value::int(grapheme_idx as i64));
                }
                byte_idx += g.len();
            }
            // byte_pos pointed to end of string
            (SIG_OK, Value::NIL)
        }
        None => (SIG_OK, Value::NIL),
    }
}

/// Split string or @string on delimiter, returning an array
pub(crate) fn prim_string_split(args: &[Value]) -> (SignalBits, Value) {
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

    let delimiter = if let Some(d) = args[1].with_string(|s| s.to_string()) {
        d
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("string-split: expected string, got {}", args[1].type_name()),
            ),
        );
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

    let parts: Vec<Value> = s.split(&delimiter).map(Value::string).collect();

    (SIG_OK, Value::array(parts))
}

/// Replace all occurrences of old with new in a string or buffer
pub(crate) fn prim_string_replace(args: &[Value]) -> (SignalBits, Value) {
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

    let old = if let Some(o) = args[1].with_string(|s| s.to_string()) {
        o
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-replace: expected string, got {}",
                    args[1].type_name()
                ),
            ),
        );
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

    let new = if let Some(n) = args[2].with_string(|s| s.to_string()) {
        n
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-replace: expected string, got {}",
                    args[2].type_name()
                ),
            ),
        );
    };

    let replaced = s.replace(&*old, &new);
    if is_buffer {
        (SIG_OK, Value::string_mut(replaced.into_bytes()))
    } else {
        (SIG_OK, Value::string(replaced))
    }
}

/// Trim leading and trailing whitespace from a string or buffer
pub(crate) fn prim_string_trim(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::string_mut(trimmed.into_bytes()))
    } else {
        (SIG_OK, Value::string(trimmed))
    }
}

/// Check if string or buffer contains substring
pub(crate) fn prim_string_contains(args: &[Value]) -> (SignalBits, Value) {
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

    let needle = if let Some(n) = args[1].with_string(|s| s.to_string()) {
        n
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-contains?: expected string, got {}",
                    args[1].type_name()
                ),
            ),
        );
    };

    (
        SIG_OK,
        if haystack.contains(&*needle) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string or buffer starts with prefix
pub(crate) fn prim_string_starts_with(args: &[Value]) -> (SignalBits, Value) {
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

    let prefix = if let Some(p) = args[1].with_string(|s| s.to_string()) {
        p
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-starts-with?: expected string, got {}",
                    args[1].type_name()
                ),
            ),
        );
    };

    (
        SIG_OK,
        if s.starts_with(&*prefix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string or buffer ends with suffix
pub(crate) fn prim_string_ends_with(args: &[Value]) -> (SignalBits, Value) {
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

    let suffix = if let Some(suf) = args[1].with_string(|s| s.to_string()) {
        suf
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string-ends-with?: expected string, got {}",
                    args[1].type_name()
                ),
            ),
        );
    };

    (
        SIG_OK,
        if s.ends_with(&*suffix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Join sequence of strings with separator
pub(crate) fn prim_string_join(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string-join: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let seq = &args[0];
    let separator = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("string-join: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    // Try tuple first
    let vec = if let Some(elems) = seq.as_array() {
        elems.to_vec()
    } else if let Some(arr) = seq.as_array_mut() {
        arr.borrow().clone()
    } else {
        // Fall back to list_to_vec for lists and syntax
        match seq.list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "string-join: expected sequence (list, tuple, or array), got {}",
                            seq.type_name()
                        ),
                    ),
                )
            }
        }
    };

    let mut strings = Vec::new();

    for val in vec {
        match val.with_string(|s| s.to_string()) {
            Some(s) => strings.push(s),
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

    (SIG_OK, Value::string(strings.join(&separator)))
}

/// Percent-encode a string per RFC 3986.
/// Unreserved characters (A-Z, a-z, 0-9, '-', '.', '_', '~') pass through.
/// All others are percent-encoded as %XX with uppercase hex.
pub(crate) fn prim_uri_encode(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("uri-encode: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        return args[0]
            .with_string(|s| {
                let mut encoded = String::with_capacity(s.len());
                for byte in s.as_bytes() {
                    match byte {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                            encoded.push(*byte as char);
                        }
                        _ => {
                            encoded.push('%');
                            encoded.push(
                                char::from_digit((*byte >> 4) as u32, 16)
                                    .unwrap()
                                    .to_ascii_uppercase(),
                            );
                            encoded.push(
                                char::from_digit((*byte & 0x0f) as u32, 16)
                                    .unwrap()
                                    .to_ascii_uppercase(),
                            );
                        }
                    }
                }
                (SIG_OK, Value::string(encoded.as_str()))
            })
            .unwrap();
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("uri-encode: expected string, got {}", args[0].type_name()),
        ),
    )
}

/// Create an @string from byte arguments
/// (@string) => empty @string
/// (@string 72 101 108) => @string with those bytes
pub(crate) fn prim_buffer(args: &[Value]) -> (SignalBits, Value) {
    let mut bytes = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => bytes.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("@string: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "@string: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::string_mut(bytes))
}

/// Return the UTF-8 byte length of a string (not grapheme count).
pub(crate) fn prim_string_size_of(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string/size-of: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(byte_len) = args[0].with_string(|s| s.len()) {
        return (SIG_OK, Value::int(byte_len as i64));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "string/size-of: expected string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Declarative primitive definitions for string module.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "@string",
        func: prim_buffer,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable string from byte arguments.",
        params: &[],
        category: "string",
        example: "(@string 72 101 108 108 111)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string/uppercase",
        func: prim_string_upcase,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert string to uppercase.",
        params: &["s"],
        category: "string",
        example: "(string/uppercase \"hello\") #=> \"HELLO\"",
        aliases: &["string/upcase", "string-upcase"],
    },
    PrimitiveDef {
        name: "string/lowercase",
        func: prim_string_downcase,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert string to lowercase.",
        params: &["s"],
        category: "string",
        example: "(string/lowercase \"HELLO\") #=> \"hello\"",
        aliases: &["string/downcase", "string-downcase"],
    },
    PrimitiveDef {
        name: "string/find",
        func: prim_string_find,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Find the grapheme index of a substring, with optional start offset.",
        params: &["haystack", "needle", "offset"],
        category: "string",
        example: "(string/find \"hello\" \"ll\") #=> 2",
        aliases: &["string-index", "string/index", "string-find"],
    },
    PrimitiveDef {
        name: "string/split",
        func: prim_string_split,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Split string by delimiter, returning an array of substrings.",
        params: &["s", "delim"],
        category: "string",
        example: "(string/split \"a,b,c\" \",\") #=> [\"a\" \"b\" \"c\"]",
        aliases: &["string-split"],
    },
    PrimitiveDef {
        name: "string/replace",
        func: prim_string_replace,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Replace all occurrences of old substring with new.",
        params: &["s", "old", "new"],
        category: "string",
        example: "(string/replace \"hello\" \"l\" \"L\") #=> \"heLLo\"",
        aliases: &["string-replace"],
    },
    PrimitiveDef {
        name: "string/trim",
        func: prim_string_trim,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove leading and trailing whitespace.",
        params: &["s"],
        category: "string",
        example: "(string/trim \"  hello  \") #=> \"hello\"",
        aliases: &["string-trim"],
    },
    PrimitiveDef {
        name: "string/contains?",
        func: prim_string_contains,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string contains substring.",
        params: &["s", "substr"],
        category: "string",
        example: "(string/contains? \"hello\" \"ell\") #=> true",
        aliases: &["string-contains?"],
    },
    PrimitiveDef {
        name: "string/starts-with?",
        func: prim_string_starts_with,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string starts with prefix.",
        params: &["s", "prefix"],
        category: "string",
        example: "(string/starts-with? \"hello\" \"he\") #=> true",
        aliases: &["string-starts-with?"],
    },
    PrimitiveDef {
        name: "string/ends-with?",
        func: prim_string_ends_with,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string ends with suffix.",
        params: &["s", "suffix"],
        category: "string",
        example: "(string/ends-with? \"hello\" \"lo\") #=> true",
        aliases: &["string-ends-with?"],
    },
    PrimitiveDef {
        name: "string/join",
        func: prim_string_join,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Join list of strings with separator.",
        params: &["lst", "sep"],
        category: "string",
        example: "(string/join (list \"a\" \"b\" \"c\") \",\") #=> \"a,b,c\"",
        aliases: &["string-join"],
    },
    PrimitiveDef {
        name: "uri-encode",
        func: prim_uri_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Percent-encode a string per RFC 3986.",
        params: &["str"],
        category: "string",
        example: "(uri-encode \"hello world\") ;=> \"hello%20world\"",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string/size-of",
        func: prim_string_size_of,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return the UTF-8 byte length of a string.",
        params: &["s"],
        category: "string",
        example: "(string/size-of \"café\") #=> 5",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_of_ascii() {
        let (sig, val) = prim_string_size_of(&[Value::string("hello")]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(5));
    }

    #[test]
    fn size_of_multibyte_utf8() {
        // "café" — 'é' is 2 bytes, so total is 5 bytes
        let (sig, val) = prim_string_size_of(&[Value::string("café")]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(5));
    }

    #[test]
    fn size_of_emoji() {
        // "🎉" is 4 bytes in UTF-8
        let (sig, val) = prim_string_size_of(&[Value::string("🎉")]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(4));
    }

    #[test]
    fn size_of_empty() {
        let (sig, val) = prim_string_size_of(&[Value::string("")]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(0));
    }

    #[test]
    fn size_of_type_error() {
        let (sig, _val) = prim_string_size_of(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }
}
