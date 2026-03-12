//! Bytes and @bytes primitives (binary data)
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Create immutable bytes from integer arguments, or from a single string/keyword.
///
/// With integer arguments: each must be 0-255, assembled into a byte sequence.
/// With a single string argument: encodes as UTF-8 bytes.
/// With a single keyword argument: converts keyword name to UTF-8 bytes.
pub(crate) fn prim_bytes(args: &[Value]) -> (SignalBits, Value) {
    // Single-argument string, @string, or keyword: convert to bytes
    if args.len() == 1 {
        // @string → @bytes (preserves mutability)
        if let Some(buf_ref) = args[0].as_string_mut() {
            return (SIG_OK, Value::bytes_mut(buf_ref.borrow().clone()));
        }
        // string → bytes (immutable)
        if let Some(data) = args[0].with_string(|s| s.as_bytes().to_vec()) {
            return (SIG_OK, Value::bytes(data));
        }
        if let Some(name) = args[0].as_keyword_name() {
            return (SIG_OK, Value::bytes(name.as_bytes().to_vec()));
        }
    }
    // Integer arguments: each must be 0-255
    let mut data = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => data.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("bytes: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "bytes: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::bytes(data))
}

/// Create mutable @bytes from integer arguments, or from a single string/keyword.
///
/// With integer arguments: each must be 0-255, assembled into a byte sequence.
/// With a single string argument: encodes as UTF-8 bytes.
/// With a single keyword argument: converts keyword name to UTF-8 bytes.
pub(crate) fn prim_blob(args: &[Value]) -> (SignalBits, Value) {
    // Single-argument string, @string, or keyword: convert to bytes
    if args.len() == 1 {
        if let Some(buf_ref) = args[0].as_string_mut() {
            return (SIG_OK, Value::bytes_mut(buf_ref.borrow().clone()));
        }
        if let Some(data) = args[0].with_string(|s| s.as_bytes().to_vec()) {
            return (SIG_OK, Value::bytes_mut(data));
        }
        if let Some(name) = args[0].as_keyword_name() {
            return (SIG_OK, Value::bytes_mut(name.as_bytes().to_vec()));
        }
    }
    // Integer arguments: each must be 0-255
    let mut data = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => data.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("@bytes: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "@bytes: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::bytes_mut(data))
}

/// bytes->hex: convert bytes or @bytes to lowercase hex string
pub(crate) fn prim_bytes_to_hex(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes->hex: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Immutable bytes
    if let Some(b) = args[0].as_bytes() {
        let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
        return (SIG_OK, Value::string(hex.as_str()));
    }
    // Mutable @bytes → mutable @string
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        let hex: String = borrowed
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect();
        return (SIG_OK, Value::string_mut(hex.into_bytes()));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "bytes->hex: expected bytes or @bytes, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Slice a sequence. Returns same type as input.
/// (slice coll start end)
///
/// Supports: bytes, @bytes, array, @array, list, string, @string.
/// Indices are 0-based, clamped to length. start >= end returns empty.
pub(crate) fn prim_slice(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("slice: expected 3 arguments, got {}", args.len()),
            ),
        );
    }
    let start = match args[1].as_int() {
        Some(i) if i >= 0 => i as usize,
        Some(i) => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("slice: start must be non-negative, got {}", i),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("slice: start must be integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let end = match args[2].as_int() {
        Some(i) if i >= 0 => i as usize,
        Some(i) => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("slice: end must be non-negative, got {}", i),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("slice: end must be integer, got {}", args[2].type_name()),
                ),
            )
        }
    };

    // Bytes (immutable)
    if let Some(b) = args[0].as_bytes() {
        let clamped_start = start.min(b.len());
        let clamped_end = end.min(b.len());
        if clamped_start >= clamped_end {
            return (SIG_OK, Value::bytes(vec![]));
        }
        return (SIG_OK, Value::bytes(b[clamped_start..clamped_end].to_vec()));
    }

    // @bytes (mutable)
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        let clamped_start = start.min(borrowed.len());
        let clamped_end = end.min(borrowed.len());
        if clamped_start >= clamped_end {
            return (SIG_OK, Value::bytes_mut(vec![]));
        }
        return (
            SIG_OK,
            Value::bytes_mut(borrowed[clamped_start..clamped_end].to_vec()),
        );
    }

    // Array (immutable)
    if let Some(elems) = args[0].as_array() {
        let clamped_start = start.min(elems.len());
        let clamped_end = end.min(elems.len());
        if clamped_start >= clamped_end {
            return (SIG_OK, Value::array(vec![]));
        }
        return (
            SIG_OK,
            Value::array(elems[clamped_start..clamped_end].to_vec()),
        );
    }

    // Array (mutable)
    if let Some(arr_ref) = args[0].as_array_mut() {
        let borrowed = arr_ref.borrow();
        let clamped_start = start.min(borrowed.len());
        let clamped_end = end.min(borrowed.len());
        if clamped_start >= clamped_end {
            return (SIG_OK, Value::array_mut(vec![]));
        }
        return (
            SIG_OK,
            Value::array_mut(borrowed[clamped_start..clamped_end].to_vec()),
        );
    }

    // String (immutable, grapheme-aware)
    if args[0].is_string() {
        return args[0]
            .with_string(|s| slice_graphemes(s, start, end, false))
            .unwrap();
    }

    // @string (mutable, grapheme-aware)
    if let Some(buf_ref) = args[0].as_string_mut() {
        let borrowed = buf_ref.borrow();
        // @string is valid UTF-8 by construction
        let s = unsafe { std::str::from_utf8_unchecked(&borrowed) };
        return slice_graphemes(s, start, end, true);
    }

    // List
    if args[0].is_empty_list() || args[0].is_cons() {
        match args[0].list_to_vec() {
            Ok(elems) => {
                let clamped_start = start.min(elems.len());
                let clamped_end = end.min(elems.len());
                if clamped_start >= clamped_end {
                    return (SIG_OK, Value::EMPTY_LIST);
                }
                let mut result = Value::EMPTY_LIST;
                for v in elems[clamped_start..clamped_end].iter().rev() {
                    result = Value::cons(*v, result);
                }
                return (SIG_OK, result);
            }
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("slice: expected proper list, got {}", args[0].type_name()),
                    ),
                );
            }
        }
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "slice: expected sequence (bytes, @bytes, array, @array, list, string, or @string), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Grapheme-aware slicing for strings and @strings.
fn slice_graphemes(s: &str, start: usize, end: usize, is_buffer: bool) -> (SignalBits, Value) {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    let clamped_start = start.min(graphemes.len());
    let clamped_end = end.min(graphemes.len());
    if clamped_start >= clamped_end {
        if is_buffer {
            return (SIG_OK, Value::string_mut(vec![]));
        } else {
            return (SIG_OK, Value::string(""));
        }
    }
    let result: String = graphemes[clamped_start..clamped_end].concat();
    if is_buffer {
        (SIG_OK, Value::string_mut(result.into_bytes()))
    } else {
        (SIG_OK, Value::string(result))
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bytes",
        func: prim_bytes,
        effect: Signal::inert(),
        arity: Arity::AtLeast(0),
        doc: "Create immutable bytes. Accepts integers (0-255), or a single string/keyword.",
        params: &[],
        category: "bytes",
        example: "(bytes 72 101 108 108 111)\n(bytes \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "@bytes",
        func: prim_blob,
        effect: Signal::inert(),
        arity: Arity::AtLeast(0),
        doc: "Create mutable bytes. Accepts integers (0-255), or a single string/keyword.",
        params: &[],
        category: "bytes",
        example: "(@bytes 72 101 108 108 111)\n(@bytes \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->hex",
        func: prim_bytes_to_hex,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Convert bytes or @bytes to a lowercase hex string.",
        params: &["b"],
        category: "bytes",
        example: "(bytes->hex (bytes 72 101 108)) ;=> \"48656c\"",
        aliases: &["bytes->hex-string"],
    },
    PrimitiveDef {
        name: "slice",
        func: prim_slice,
        effect: Signal::inert(),
        arity: Arity::Exact(3),
        doc: "Slice a sequence from start to end index. Works on bytes, @bytes, array, @array, list, string, and @string. Returns same type as input.",
        params: &["coll", "start", "end"],
        category: "bytes",
        example: "(slice [1 2 3 4 5] 1 3)",
        aliases: &[],
    },

];
