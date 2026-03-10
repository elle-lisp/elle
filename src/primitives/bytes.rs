//! Bytes and @bytes primitives (binary data)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
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
    // Single-argument string or keyword: convert to bytes
    if args.len() == 1 {
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
    // Single-argument string or keyword: convert to bytes
    if args.len() == 1 {
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

/// string->bytes: encode string as UTF-8 bytes (immutable)
pub(crate) fn prim_string_to_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string->bytes: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(bytes) = args[0].with_string(|s| s.as_bytes().to_vec()) {
        (SIG_OK, Value::bytes(bytes))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string->bytes: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// string->blob: encode string as UTF-8 @bytes (mutable)
pub(crate) fn prim_string_to_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string->blob: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(bytes) = args[0].with_string(|s| s.as_bytes().to_vec()) {
        (SIG_OK, Value::bytes_mut(bytes))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("string->blob: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// bytes->string: decode UTF-8 bytes to string (fallible)
pub(crate) fn prim_bytes_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes() {
        Some(b) => match std::str::from_utf8(b) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("bytes->string: invalid UTF-8: {}", e)),
            ),
        },
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bytes->string: expected bytes, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// blob->string: decode UTF-8 @bytes to string (fallible)
pub(crate) fn prim_blob_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes_mut() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            match std::str::from_utf8(&borrowed) {
                Ok(s) => (SIG_OK, Value::string(s)),
                Err(e) => (
                    SIG_ERROR,
                    error_val("error", format!("blob->string: invalid UTF-8: {}", e)),
                ),
            }
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("blob->string: expected blob, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// bytes->hex: convert bytes to lowercase hex string
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
    match args[0].as_bytes() {
        Some(b) => {
            let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
            (SIG_OK, Value::string(hex.as_str()))
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bytes->hex: expected bytes, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// blob->hex: convert @bytes to lowercase hex string
pub(crate) fn prim_blob_to_hex(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->hex: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes_mut() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            let hex: String = borrowed
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect();
            (SIG_OK, Value::string(hex.as_str()))
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("blob->hex: expected blob, got {}", args[0].type_name()),
            ),
        ),
    }
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

    // Blob (mutable)
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

    // Tuple (immutable)
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

    // Buffer (mutable, grapheme-aware)
    if let Some(buf_ref) = args[0].as_string_mut() {
        let borrowed = buf_ref.borrow();
        // Buffer is valid UTF-8 by construction
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

/// buffer->bytes: convert @string to immutable bytes
pub(crate) fn prim_buffer_to_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer->bytes: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_string_mut() {
        Some(buf_ref) => (SIG_OK, Value::bytes(buf_ref.borrow().clone())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "buffer->bytes: expected buffer, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// buffer->blob: convert @string to mutable @bytes
pub(crate) fn prim_buffer_to_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer->blob: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_string_mut() {
        Some(buf_ref) => (SIG_OK, Value::bytes_mut(buf_ref.borrow().clone())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("buffer->blob: expected buffer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// bytes->buffer: convert bytes to @string. Error on invalid UTF-8.
pub(crate) fn prim_bytes_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes->buffer: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes() {
        Some(b) => match std::str::from_utf8(b) {
            Ok(_) => (SIG_OK, Value::string_mut(b.to_vec())),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("bytes->buffer: invalid UTF-8: {}", e)),
            ),
        },
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bytes->buffer: expected bytes, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// blob->buffer: convert @bytes to @string. Error on invalid UTF-8.
pub(crate) fn prim_blob_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->buffer: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes_mut() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            match std::str::from_utf8(&borrowed) {
                Ok(_) => (SIG_OK, Value::string_mut(borrowed.clone())),
                Err(e) => (
                    SIG_ERROR,
                    error_val("error", format!("blob->buffer: invalid UTF-8: {}", e)),
                ),
            }
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("blob->buffer: expected blob, got {}", args[0].type_name()),
            ),
        ),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bytes",
        func: prim_bytes,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::AtLeast(0),
        doc: "Create mutable bytes. Accepts integers (0-255), or a single string/keyword.",
        params: &[],
        category: "bytes",
        example: "(@bytes 72 101 108 108 111)\n(@bytes \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->bytes",
        func: prim_string_to_bytes,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Encode a string as immutable UTF-8 bytes.",
        params: &["str"],
        category: "bytes",
        example: "(string->bytes \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->blob",
        func: prim_string_to_blob,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Encode a string as mutable UTF-8 @bytes.",
        params: &["str"],
        category: "bytes",
        example: "(string->blob \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->string",
        func: prim_bytes_to_string,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Decode UTF-8 bytes to a string. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(bytes->string (bytes 104 105))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob->string",
        func: prim_blob_to_string,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Decode UTF-8 @bytes to a string. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(blob->string (blob 104 105))",
        aliases: &[],
    },

    PrimitiveDef {
        name: "bytes->hex",
        func: prim_bytes_to_hex,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert bytes to a lowercase hex string.",
        params: &["b"],
        category: "bytes",
        example: "(bytes->hex (bytes 72 101 108)) ;=> \"48656c\"",
        aliases: &["bytes->hex-string"],
    },
    PrimitiveDef {
        name: "blob->hex",
        func: prim_blob_to_hex,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert @bytes to a lowercase hex string.",
        params: &["b"],
        category: "bytes",
        example: "(blob->hex (blob 72 101 108)) ;=> \"48656c\"",
        aliases: &["blob->hex-string"],
    },
    PrimitiveDef {
        name: "slice",
        func: prim_slice,
        effect: Effect::inert(),
        arity: Arity::Exact(3),
        doc: "Slice a sequence from start to end index. Works on bytes, @bytes, array, @array, list, string, and @string. Returns same type as input.",
        params: &["coll", "start", "end"],
        category: "bytes",
        example: "(slice [1 2 3 4 5] 1 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer->bytes",
        func: prim_buffer_to_bytes,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert @string to immutable bytes.",
        params: &["buf"],
        category: "bytes",
        example: "(buffer->bytes @\"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer->blob",
        func: prim_buffer_to_blob,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert @string to mutable @bytes.",
        params: &["buf"],
        category: "bytes",
        example: "(buffer->blob @\"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->buffer",
        func: prim_bytes_to_buffer,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert bytes to @string. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(bytes->buffer (bytes 104 105))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob->buffer",
        func: prim_blob_to_buffer,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert @bytes to @string. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(blob->buffer (blob 104 105))",
        aliases: &[],
    },
];
