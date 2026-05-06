//! Bytes and @bytes primitives (binary data)
use crate::primitives::def::PrimitiveDef;
use crate::primitives::seq::seq_slice;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create immutable bytes from integer arguments, or from a single string/keyword.
///
/// With integer arguments: each must be 0-255, assembled into a byte sequence.
/// With a single string argument: encodes as UTF-8 bytes.
/// With a single keyword argument: converts keyword name to UTF-8 bytes.
pub(crate) fn prim_bytes(args: &[Value]) -> (SignalBits, Value) {
    // Single-argument coercion: string/keyword → bytes, bytes → passthrough
    if args.len() == 1 {
        // bytes → bytes (idempotent)
        if args[0].as_bytes().is_some() {
            return (SIG_OK, args[0]);
        }
        // @bytes → @bytes (idempotent)
        if args[0].as_bytes_mut().is_some() {
            return (SIG_OK, args[0]);
        }
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
                        "argument-error",
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
    // Single-argument coercion: string/keyword → @bytes, bytes → passthrough
    if args.len() == 1 {
        // @bytes → @bytes (idempotent)
        if args[0].as_bytes_mut().is_some() {
            return (SIG_OK, args[0]);
        }
        // bytes → bytes (idempotent, preserves immutability)
        if args[0].as_bytes().is_some() {
            return (SIG_OK, args[0]);
        }
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
                        "argument-error",
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

/// Encode a byte slice as a lowercase hex string (2 chars per byte).
fn bytes_to_hex_string(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{:02x}", byte)).collect()
}

/// Validate and collect byte values from an iterator of `Value`.
///
/// Returns `Ok(Vec<u8>)` on success, or `Err((SignalBits, Value))` on the
/// first invalid element. `idx` is the position label used in error messages.
fn collect_byte_values<I>(iter: I) -> Result<Vec<u8>, (SignalBits, Value)>
where
    I: IntoIterator<Item = Value>,
{
    let mut out = Vec::new();
    for (i, v) in iter.into_iter().enumerate() {
        match v.as_int() {
            Some(n) if (0..=255).contains(&n) => out.push(n as u8),
            Some(n) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("seq->hex: byte at index {} out of range 0-255: {}", i, n),
                    ),
                ));
            }
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "seq->hex: element at index {} must be integer, got {}",
                            i,
                            v.type_name()
                        ),
                    ),
                ));
            }
        }
    }
    Ok(out)
}

/// seq->hex: convert bytes, @bytes, array, @array, list, or integer to a
/// lowercase hex string.
///
/// Mutability rule: mutable input (@bytes, @array) → @string.
/// Everything else (bytes, array, list, integer) → string.
///
/// Integer input: big-endian, minimal bytes, no leading zero bytes except
/// that 0 itself produces "00".  Negative integers → value-error.
pub(crate) fn prim_seq_to_hex(args: &[Value]) -> (SignalBits, Value) {
    // Immutable bytes → immutable string
    if let Some(b) = args[0].as_bytes() {
        return (SIG_OK, Value::string(bytes_to_hex_string(b)));
    }

    // Mutable @bytes → mutable @string
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        let hex = bytes_to_hex_string(&borrowed);
        return (SIG_OK, Value::string_mut(hex.into_bytes()));
    }

    // Integer: big-endian minimal bytes, at least 1 byte
    if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("seq->hex: negative integers not supported, got {}", n),
                ),
            );
        }
        // Extract big-endian bytes and strip leading zeros, keeping at least 1 byte.
        let be = (n as u64).to_be_bytes();
        let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
        return (SIG_OK, Value::string(bytes_to_hex_string(&be[start..])));
    }

    // Immutable array → immutable string
    if let Some(elems) = args[0].as_array() {
        return match collect_byte_values(elems.iter().copied()) {
            Ok(bytes) => (SIG_OK, Value::string(bytes_to_hex_string(&bytes))),
            Err(e) => e,
        };
    }

    // Mutable @array → mutable @string
    if let Some(arr_ref) = args[0].as_array_mut() {
        let borrowed = arr_ref.borrow();
        return match collect_byte_values(borrowed.iter().copied()) {
            Ok(bytes) => {
                let hex = bytes_to_hex_string(&bytes);
                (SIG_OK, Value::string_mut(hex.into_bytes()))
            }
            Err(e) => e,
        };
    }

    // List (always immutable in Elle) → immutable string
    if args[0].is_empty_list() || args[0].is_pair() {
        return match args[0].list_to_vec() {
            Ok(elems) => match collect_byte_values(elems) {
                Ok(bytes) => (SIG_OK, Value::string(bytes_to_hex_string(&bytes))),
                Err(e) => e,
            },
            Err(_) => (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "seq->hex: expected proper list, got {}",
                        args[0].type_name()
                    ),
                ),
            ),
        };
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "seq->hex: expected bytes, array, list, or integer, got {}",
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
    let raw_start = match args[1].as_int() {
        Some(i) => i,
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

    // If no end provided, use i64::MAX (will be clamped to len below)
    let raw_end = if args.len() == 2 {
        i64::MAX
    } else {
        match args[2].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("slice: end must be integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    };

    match seq_slice(&args[0], raw_start, raw_end) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "bytes",
        func: prim_bytes,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create mutable bytes. Accepts integers (0-255), or a single string/keyword.",
        params: &[],
        category: "bytes",
        example: "(@bytes 72 101 108 108 111)\n(@bytes \"hello\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "seq->hex",
        func: prim_seq_to_hex,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert bytes, @bytes, array, @array, list, or integer to a lowercase hex string. Mutable input (@bytes, @array) produces @string; all other input produces string. Integer input uses big-endian minimal-byte encoding (0 → \"00\"). Aliases: bytes->hex, bytes->hex-string.",
        params: &["x"],
        category: "bytes",
        example: "(seq->hex (bytes 72 101 108)) ;=> \"48656c\"\n(seq->hex [72 101 108]) ;=> \"48656c\"\n(seq->hex 255) ;=> \"ff\"",
        aliases: &["bytes->hex", "bytes->hex-string"],
    },
    PrimitiveDef {
        name: "slice",
        func: prim_slice,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Slice a sequence from start to end index. If end is omitted, slice to end of sequence. Returns same type as input.",
        params: &["coll", "start", "end?"],
        category: "bytes",
        example: "(slice [1 2 3 4 5] 1 3)\n(slice \"hello\" 1)",
        aliases: &[],
    },

];
