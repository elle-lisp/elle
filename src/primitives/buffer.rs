//! Buffer primitives (mutable byte sequences)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create a buffer from byte arguments
/// (buffer) => empty buffer
/// (buffer 72 101 108) => buffer with those bytes
pub fn prim_buffer(args: &[Value]) -> (SignalBits, Value) {
    let mut bytes = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => bytes.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("buffer: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "buffer: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::buffer(bytes))
}

/// Convert a string to a buffer (UTF-8 bytes)
pub fn prim_string_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string->buffer: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(bytes) = args[0].with_string(|s| s.as_bytes().to_vec()) {
        (SIG_OK, Value::buffer(bytes))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string->buffer: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Convert a buffer to a string (UTF-8)
pub fn prim_buffer_to_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer->string: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_buffer() {
        Some(buf_ref) => {
            let borrowed = buf_ref.borrow();
            match String::from_utf8(borrowed.clone()) {
                Ok(s) => (SIG_OK, Value::string(s.as_str())),
                Err(e) => (
                    SIG_ERROR,
                    error_val("error", format!("buffer->string: invalid UTF-8: {}", e)),
                ),
            }
        }
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "buffer->string: expected buffer, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "buffer",
        func: prim_buffer,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable buffer from byte arguments.",
        params: &[],
        category: "buffer",
        example: "(buffer 72 101 108 108 111)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->buffer",
        func: prim_string_to_buffer,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a buffer (UTF-8 bytes).",
        params: &["str"],
        category: "buffer",
        example: "(string->buffer \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer->string",
        func: prim_buffer_to_string,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert a buffer to a string (UTF-8).",
        params: &["buf"],
        category: "buffer",
        example: "(buffer->string @\"hello\")",
        aliases: &[],
    },
];
