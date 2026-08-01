//! String manipulation primitives
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use unicode_segmentation::UnicodeSegmentation;

mod ops;
pub(crate) use ops::*;

/// Extract text content from a string or @string value.
/// Returns (text, is_@string). For @strings, validates UTF-8.
fn as_text(
    val: &Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<(String, bool), (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_string()) {
        Ok((s, false))
    } else if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        match String::from_utf8(borrowed.clone()) {
            Ok(s) => Ok((s, true)),
            Err(e) => Err((
                SIG_ERROR,
                ctx.error(
                    "encoding-error",
                    format!("{}: buffer contains invalid UTF-8: {}", prim_name, e),
                ),
            )),
        }
    } else {
        Err((
            SIG_ERROR,
            ctx.error(
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
pub(crate) fn prim_string_upcase(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, is_buffer) = match as_text(&args[0], "string-upcase", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let upper = s.to_uppercase();
    if is_buffer {
        (SIG_OK, ctx.string_mut(upper.into_bytes()))
    } else {
        (SIG_OK, ctx.string(upper))
    }
}

/// Convert string or buffer to lowercase
pub(crate) fn prim_string_downcase(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, is_buffer) = match as_text(&args[0], "string-downcase", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let lower = s.to_lowercase();
    if is_buffer {
        (SIG_OK, ctx.string_mut(lower.into_bytes()))
    } else {
        (SIG_OK, ctx.string(lower))
    }
}

/// Find the grapheme index of a substring, with optional start offset
pub(crate) fn prim_string_find(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (haystack, _is_buffer) = match as_text(&args[0], "string/find", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let needle = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return type_error!(ctx, args[1], "string/find", "string"),
    };

    let offset = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) if n >= 0 => n as usize,
            Some(_) => return (SIG_OK, Value::NIL),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
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

/// Percent-encode a string per RFC 3986.
/// Unreserved characters (A-Z, a-z, 0-9, '-', '.', '_', '~') pass through.
/// All others are percent-encoded as %XX with uppercase hex.
pub(crate) fn prim_uri_encode(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
                (SIG_OK, ctx.string(encoded.as_str()))
            })
            .unwrap();
    }
    type_error!(ctx, args[0], "uri-encode", "string")
}

/// Create an @string from byte integers, strings, or @strings.
/// (@string) => empty @string
/// (@string 72 101 108) => @string with those bytes
/// (@string "hello" " " "world") => @string with concatenated UTF-8 bytes
pub(crate) fn prim_string_mut(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let mut bytes = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        if let Some(s) = arg.with_string(|s| s.as_bytes().to_vec()) {
            bytes.extend(s);
        } else if let Some(buf_ref) = arg.as_string_mut() {
            bytes.extend(buf_ref.borrow().iter());
        } else {
            match arg.as_int() {
                Some(n) if (0..=255).contains(&n) => bytes.push(n as u8),
                Some(n) => {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "argument-error",
                            format!("@string: byte {} out of range 0-255: {}", i, n),
                        ),
                    )
                }
                None => {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "type-error",
                            format!(
                            "@string: expected integer, string, or @string, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                        ),
                    )
                }
            }
        }
    }
    (SIG_OK, ctx.string_mut(bytes))
}

/// Return the UTF-8 byte length of a string (not grapheme count).
pub(crate) fn prim_string_size_of(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(byte_len) = args[0].with_string(|s| s.len()) {
        return (SIG_OK, Value::int(byte_len as i64));
    }
    if let Some(buf_ref) = args[0].as_string_mut() {
        return (SIG_OK, Value::int(buf_ref.borrow().len() as i64));
    }
    type_error!(ctx, args[0], "string/size-of", "string or @string")
}

/// Repeat a string N times
pub(crate) fn prim_string_repeat(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let s = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "string/repeat", "string");
    };
    let n = if let Some(i) = args[1].as_int() {
        if i < 0 {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "string/repeat: count must be non-negative".to_string(),
                ),
            );
        }
        i as usize
    } else {
        return type_error!(ctx, args[1], "string/repeat", "integer count");
    };
    (SIG_OK, ctx.string(s.repeat(n)))
}

// Declarative primitive definitions for string module.
primitive! {
    "@string" => prim_string_mut {
        ret: RetType::MutableString,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable string from byte arguments.",
        category: "string",
        example: "(@string 72 101 108 108 111)",
        effect: RegionEffect::Fresh,
    }
    "string/uppercase" => prim_string_upcase {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert string to uppercase.",
        params: &["s"],
        category: "string",
        example: "(string/uppercase \"hello\") #=> \"HELLO\"",
        aliases: &["string/upcase", "string-upcase"],
        effect: RegionEffect::Fresh,
    }
    "string/lowercase" => prim_string_downcase {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert string to lowercase.",
        params: &["s"],
        category: "string",
        example: "(string/lowercase \"HELLO\") #=> \"hello\"",
        aliases: &["string/downcase", "string-downcase"],
        effect: RegionEffect::Fresh,
    }
    "string/find" => prim_string_find {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Find the grapheme index of a substring, with optional start offset. Returns the index (integer) or nil if not found.",
        params: &["haystack", "needle", "offset"],
        category: "string",
        example: "(string/find \"hello\" \"ll\") #=> 2",
        aliases: &[
            "string-index",
            "string/index",
            "string-find",
            "string/index-of",
        ],
        effect: RegionEffect::Immediate,
    }
    "string/split" => prim_string_split {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Split string by delimiter. Returns an array of substrings (empty strings between consecutive delimiters). Delimiter cannot be empty.",
        params: &["s", "delim"],
        category: "string",
        example: "(string/split \"a,b,c\" \",\") #=> [\"a\" \"b\" \"c\"]",
        aliases: &["string-split"],
        effect: RegionEffect::Fresh,
    }
    "string/replace" => prim_string_replace {
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Replace all occurrences of old substring with new.",
        params: &["s", "old", "new"],
        category: "string",
        example: "(string/replace \"hello\" \"l\" \"L\") #=> \"heLLo\"",
        aliases: &["string-replace"],
        effect: RegionEffect::Fresh,
    }
    "string/trim" => prim_string_trim {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove leading and trailing whitespace.",
        params: &["s"],
        category: "string",
        example: "(string/trim \"  hello  \") #=> \"hello\"",
        aliases: &["string-trim"],
        effect: RegionEffect::Fresh,
    }
    "string/contains?" => prim_string_contains {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string contains substring.",
        params: &["s", "substr"],
        category: "string",
        example: "(string/contains? \"hello\" \"ell\") #=> true",
        effect: RegionEffect::Immediate,
    }
    "string/starts-with?" => prim_string_starts_with {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string starts with prefix.",
        params: &["s", "prefix"],
        category: "string",
        example: "(string/starts-with? \"hello\" \"he\") #=> true",
        aliases: &["string-starts-with?"],
        effect: RegionEffect::Immediate,
    }
    "string/ends-with?" => prim_string_ends_with {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if string ends with suffix.",
        params: &["s", "suffix"],
        category: "string",
        example: "(string/ends-with? \"hello\" \"lo\") #=> true",
        aliases: &["string-ends-with?"],
        effect: RegionEffect::Immediate,
    }
    "string/join" => prim_string_join {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Join list of strings with separator.",
        params: &["lst", "sep"],
        category: "string",
        example: "(string/join (list \"a\" \"b\" \"c\") \",\") #=> \"a,b,c\"",
        aliases: &["string-join"],
        effect: RegionEffect::Fresh,
    }
    "uri-encode" => prim_uri_encode {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Percent-encode a string per RFC 3986.",
        params: &["str"],
        category: "string",
        example: "(uri-encode \"hello world\") ;=> \"hello%20world\"",
        effect: RegionEffect::Fresh,
    }
    "string/size-of" => prim_string_size_of {
        arity: Arity::Exact(1),
        doc: "Return the UTF-8 byte length of a string.",
        params: &["s"],
        category: "string",
        example: "(string/size-of \"café\") #=> 5",
        effect: RegionEffect::Immediate,
    }
    "string/repeat" => prim_string_repeat {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Repeat a string N times.",
        params: &["s", "n"],
        category: "string",
        example: "(string/repeat \"ab\" 3) #=> \"ababab\"",
        aliases: &["string-repeat"],
        effect: RegionEffect::Fresh,
    }
}

// Tests migrated to tests/elle/prim-string.lisp
