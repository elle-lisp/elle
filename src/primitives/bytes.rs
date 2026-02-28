//! Bytes and blob primitives (binary data)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create immutable bytes from integer arguments
pub fn prim_bytes(args: &[Value]) -> (SignalBits, Value) {
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

/// Create mutable blob from integer arguments
pub fn prim_blob(args: &[Value]) -> (SignalBits, Value) {
    let mut data = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => data.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("blob: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "blob: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::blob(data))
}

/// string->bytes: encode string as UTF-8 bytes (immutable)
pub fn prim_string_to_bytes(args: &[Value]) -> (SignalBits, Value) {
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

/// string->blob: encode string as UTF-8 blob (mutable)
pub fn prim_string_to_blob(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::blob(bytes))
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
pub fn prim_bytes_to_string(args: &[Value]) -> (SignalBits, Value) {
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

/// blob->string: decode UTF-8 blob to string (fallible)
pub fn prim_blob_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_blob() {
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

/// blob->bytes: freeze blob to immutable bytes (copies)
pub fn prim_blob_to_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->bytes: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_blob() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            (SIG_OK, Value::bytes(borrowed.clone()))
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("blob->bytes: expected blob, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// bytes->blob: thaw bytes to mutable blob (copies)
pub fn prim_bytes_to_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes->blob: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_bytes() {
        Some(b) => (SIG_OK, Value::blob(b.to_vec())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("bytes->blob: expected bytes, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// bytes->hex: convert bytes to lowercase hex string
pub fn prim_bytes_to_hex(args: &[Value]) -> (SignalBits, Value) {
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

/// blob->hex: convert blob to lowercase hex string
pub fn prim_blob_to_hex(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->hex: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_blob() {
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

/// Append a byte (int 0-255) to a blob. Returns the blob.
pub fn prim_blob_push(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob/push: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("blob/push: expected blob, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let byte = match args[1].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("blob/push: byte out of range 0-255: {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("blob/push: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    blob_ref.borrow_mut().push(byte);
    (SIG_OK, args[0])
}

/// Remove and return last byte from blob as int. Error on empty.
pub fn prim_blob_pop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob/pop: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("blob/pop: expected blob, got {}", args[0].type_name()),
                ),
            )
        }
    };
    match blob_ref.borrow_mut().pop() {
        Some(byte) => (SIG_OK, Value::int(byte as i64)),
        None => (
            SIG_ERROR,
            error_val("error", "blob/pop: empty blob".to_string()),
        ),
    }
}

/// Set byte at index in blob. Error on OOB.
pub fn prim_blob_put(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob/put: expected 3 arguments, got {}", args.len()),
            ),
        );
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("blob/put: expected blob, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "blob/put: index must be integer, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let byte = match args[2].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val("error", format!("blob/put: byte out of range 0-255: {}", n)),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("blob/put: expected integer, got {}", args[2].type_name()),
                ),
            )
        }
    };
    let len = blob_ref.borrow().len();
    if index < 0 || (index as usize) >= len {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!("blob/put: index {} out of bounds (length {})", index, len),
            ),
        );
    }
    blob_ref.borrow_mut()[index as usize] = byte;
    (SIG_OK, args[0])
}

/// Slice a bytes or blob. Returns same type as input.
/// (slice coll start end)
pub fn prim_slice(args: &[Value]) -> (SignalBits, Value) {
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
    if let Some(b) = args[0].as_bytes() {
        let clamped_start = start.min(b.len());
        let clamped_end = end.min(b.len());
        if clamped_start > clamped_end {
            return (SIG_OK, Value::bytes(vec![]));
        }
        return (SIG_OK, Value::bytes(b[clamped_start..clamped_end].to_vec()));
    }
    if let Some(blob_ref) = args[0].as_blob() {
        let borrowed = blob_ref.borrow();
        let clamped_start = start.min(borrowed.len());
        let clamped_end = end.min(borrowed.len());
        if clamped_start > clamped_end {
            return (SIG_OK, Value::blob(vec![]));
        }
        return (
            SIG_OK,
            Value::blob(borrowed[clamped_start..clamped_end].to_vec()),
        );
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("slice: expected bytes or blob, got {}", args[0].type_name()),
        ),
    )
}

/// buffer->bytes: convert buffer to immutable bytes
pub fn prim_buffer_to_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer->bytes: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_buffer() {
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

/// buffer->blob: convert buffer to mutable blob
pub fn prim_buffer_to_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer->blob: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_buffer() {
        Some(buf_ref) => (SIG_OK, Value::blob(buf_ref.borrow().clone())),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("buffer->blob: expected buffer, got {}", args[0].type_name()),
            ),
        ),
    }
}

/// bytes->buffer: convert bytes to buffer. Error on invalid UTF-8.
pub fn prim_bytes_to_buffer(args: &[Value]) -> (SignalBits, Value) {
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
            Ok(_) => (SIG_OK, Value::buffer(b.to_vec())),
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

/// blob->buffer: convert blob to buffer. Error on invalid UTF-8.
pub fn prim_blob_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob->buffer: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_blob() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            match std::str::from_utf8(&borrowed) {
                Ok(_) => (SIG_OK, Value::buffer(borrowed.clone())),
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

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bytes",
        func: prim_bytes,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create immutable bytes from integer arguments (0-255).",
        params: &[],
        category: "bytes",
        example: "(bytes 72 101 108 108 111)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob",
        func: prim_blob,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable blob from integer arguments (0-255).",
        params: &[],
        category: "bytes",
        example: "(blob 72 101 108 108 111)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->bytes",
        func: prim_string_to_bytes,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Encode a string as a mutable UTF-8 blob.",
        params: &["str"],
        category: "bytes",
        example: "(string->blob \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->string",
        func: prim_bytes_to_string,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Decode a UTF-8 blob to a string. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(blob->string (blob 104 105))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob->bytes",
        func: prim_blob_to_bytes,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Freeze a blob to immutable bytes (copies).",
        params: &["b"],
        category: "bytes",
        example: "(blob->bytes (blob 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->blob",
        func: prim_bytes_to_blob,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Thaw bytes to a mutable blob (copies).",
        params: &["b"],
        category: "bytes",
        example: "(bytes->blob (bytes 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->hex",
        func: prim_bytes_to_hex,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert a blob to a lowercase hex string.",
        params: &["b"],
        category: "bytes",
        example: "(blob->hex (blob 72 101 108)) ;=> \"48656c\"",
        aliases: &["blob->hex-string"],
    },
    PrimitiveDef {
        name: "slice",
        func: prim_slice,
        effect: Effect::none(),
        arity: Arity::Exact(3),
        doc: "Slice bytes or blob from start to end index.",
        params: &["coll", "start", "end"],
        category: "bytes",
        example: "(slice (bytes 1 2 3 4 5) 1 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer->bytes",
        func: prim_buffer_to_bytes,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert buffer to immutable bytes.",
        params: &["buf"],
        category: "bytes",
        example: "(buffer->bytes @\"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer->blob",
        func: prim_buffer_to_blob,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert buffer to mutable blob.",
        params: &["buf"],
        category: "bytes",
        example: "(buffer->blob @\"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes->buffer",
        func: prim_bytes_to_buffer,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert bytes to buffer. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(bytes->buffer (bytes 104 105))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob->buffer",
        func: prim_blob_to_buffer,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert blob to buffer. Errors on invalid UTF-8.",
        params: &["b"],
        category: "bytes",
        example: "(blob->buffer (blob 104 105))",
        aliases: &[],
    },
];
