// audited: 2026-10-20
//! The bytes and @bytes primitives: construct binary data, render it as hex, slice a sequence, and find a needle.
//!
//! docs/bytes.md

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::{RegionEffect, RetType};
use crate::primitives::seq::seq_slice;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Create immutable bytes from integer arguments, or from a single string/keyword.
///
/// With integer arguments: each must be 0-255, assembled into a byte sequence.
/// With a single string argument: encodes as UTF-8 bytes.
/// With a single keyword argument: converts keyword name to UTF-8 bytes.
pub(crate) fn prim_bytes(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Single-argument coercion. `bytes` ALWAYS yields immutable: the
    // constructor's mutability is its contract, overriding the argument's (a
    // result that inherited the argument's mutability would make `(bytes @x)`
    // mutable — inverting the name; pinned by tests/lang/bytes.lisp).
    if args.len() == 1 {
        // bytes → bytes (already immutable: share, no copy)
        if args[0].as_bytes().is_some() {
            return (SIG_OK, args[0]);
        }
        // @bytes → bytes (copy to immutable)
        if let Some(blob_ref) = args[0].as_bytes_mut() {
            return (SIG_OK, ctx.bytes(blob_ref.borrow().clone()));
        }
        // @string → bytes (immutable)
        if let Some(buf_ref) = args[0].as_string_mut() {
            return (SIG_OK, ctx.bytes(buf_ref.borrow().clone()));
        }
        // string → bytes (immutable)
        if let Some(data) = args[0].with_string(|s| s.as_bytes().to_vec()) {
            return (SIG_OK, ctx.bytes(data));
        }
        if let Some(name) = ctx.keyword_spelling(args[0]) {
            return (SIG_OK, ctx.bytes(name.as_bytes().to_vec()));
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
                    ctx.error(
                        "argument-error",
                        format!("bytes: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
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
    (SIG_OK, ctx.bytes(data))
}

/// Create mutable @bytes from integer arguments, or from a single string/keyword.
///
/// With integer arguments: each must be 0-255, assembled into a byte sequence.
/// With a single string argument: encodes as UTF-8 bytes.
/// With a single keyword argument: converts keyword name to UTF-8 bytes.
pub(crate) fn prim_bytes_mut(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Single-argument coercion. `@bytes` ALWAYS yields mutable: the
    // constructor's mutability overrides the argument's (a result that inherited
    // the argument's mutability would make `(@bytes x)` immutable — inverting the
    // name; pinned by tests/lang/bytes.lisp).
    if args.len() == 1 {
        // @bytes → @bytes (already mutable: share, no copy)
        if args[0].as_bytes_mut().is_some() {
            return (SIG_OK, args[0]);
        }
        // bytes → @bytes (copy to mutable)
        if let Some(b) = args[0].as_bytes() {
            return (SIG_OK, ctx.bytes_mut(b.to_vec()));
        }
        if let Some(buf_ref) = args[0].as_string_mut() {
            return (SIG_OK, ctx.bytes_mut(buf_ref.borrow().clone()));
        }
        if let Some(data) = args[0].with_string(|s| s.as_bytes().to_vec()) {
            return (SIG_OK, ctx.bytes_mut(data));
        }
        if let Some(name) = ctx.keyword_spelling(args[0]) {
            return (SIG_OK, ctx.bytes_mut(name.as_bytes().to_vec()));
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
                    ctx.error(
                        "argument-error",
                        format!("@bytes: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
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
    (SIG_OK, ctx.bytes_mut(data))
}

/// Encode a byte slice as a lowercase hex string (2 chars per byte).
fn bytes_to_hex_string(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{:02x}", byte)).collect()
}

/// Validate and collect byte values from an iterator of `Value`.
///
/// Returns `Ok(Vec<u8>)` on success, or `Err((SignalBits, Value))` on the
/// first invalid element, whose index the error names.
fn collect_byte_values<I>(iter: I, ctx: &mut NativeCtx) -> Result<Vec<u8>, (SignalBits, Value)>
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
                    ctx.error(
                        "value-error",
                        format!("seq->hex: byte at index {} out of range 0-255: {}", i, n),
                    ),
                ));
            }
            None => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
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
pub(crate) fn prim_seq_to_hex(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Immutable bytes → immutable string
    if let Some(b) = args[0].as_bytes() {
        return (SIG_OK, ctx.string(bytes_to_hex_string(b)));
    }

    // Mutable @bytes → mutable @string
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        let hex = bytes_to_hex_string(&borrowed);
        return (SIG_OK, ctx.string_mut(hex.into_bytes()));
    }

    // Integer: big-endian minimal bytes, at least 1 byte
    if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("seq->hex: negative integers not supported, got {}", n),
                ),
            );
        }
        // Extract big-endian bytes and strip leading zeros, keeping at least 1 byte.
        let be = (n as u64).to_be_bytes();
        let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
        return (SIG_OK, ctx.string(bytes_to_hex_string(&be[start..])));
    }

    // Immutable array → immutable string
    if let Some(elems) = args[0].as_array() {
        return match collect_byte_values(elems.iter().copied(), ctx) {
            Ok(bytes) => (SIG_OK, ctx.string(bytes_to_hex_string(&bytes))),
            Err(e) => e,
        };
    }

    // Mutable @array → mutable @string
    if let Some(arr_ref) = args[0].as_array_mut() {
        let borrowed = arr_ref.borrow();
        return match collect_byte_values(borrowed.iter().copied(), ctx) {
            Ok(bytes) => {
                let hex = bytes_to_hex_string(&bytes);
                (SIG_OK, ctx.string_mut(hex.into_bytes()))
            }
            Err(e) => e,
        };
    }

    // List (always immutable in Elle) → immutable string
    if args[0].is_empty_list() || args[0].is_pair() {
        return match args[0].list_to_vec() {
            Ok(elems) => match collect_byte_values(elems, ctx) {
                Ok(bytes) => (SIG_OK, ctx.string(bytes_to_hex_string(&bytes))),
                Err(e) => e,
            },
            Err(_) => type_error!(ctx, args[0], "seq->hex", "proper list"),
        };
    }

    type_error!(ctx, args[0], "seq->hex", "bytes, array, list, or integer")
}

/// Slice a sequence. Returns same type as input.
/// (slice coll start end)
///
/// Supports: bytes, @bytes, array, @array, list, string, @string.
/// Indices are 0-based, clamped to length. start >= end returns empty.
pub(crate) fn prim_slice(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let raw_start = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
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
                    ctx.error(
                        "type-error",
                        format!("slice: end must be integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    };

    match seq_slice(&args[0], raw_start, raw_end, ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Run `f` over the bytes `v` holds, whichever of the four byte-carrying
/// types it is. `None` for any other type.
fn with_byte_content<R>(v: &Value, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    if let Some(b) = v.as_bytes() {
        return Some(f(b));
    }
    if let Some(s) = v.as_str() {
        return Some(f(s.as_bytes()));
    }
    if let Some(b) = v.as_bytes_mut() {
        return Some(f(&b.borrow()));
    }
    if let Some(s) = v.as_string_mut() {
        return Some(f(&s.borrow()));
    }
    None
}

/// Where `needle` first occurs in `hay`. An empty needle occurs at 0.
fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    match needle {
        [] => Some(0),
        [one] => hay.iter().position(|&x| x == *one),
        _ if needle.len() > hay.len() => None,
        _ => hay.windows(needle.len()).position(|w| w == needle),
    }
}

/// (bytes/find haystack needle \[offset\]) → int | nil
///
/// A byte index, where `string/find` answers a grapheme index. An offset past
/// the end, or below zero, answers nil rather than an error, as a needle that
/// is absent does.
pub(crate) fn prim_bytes_find(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let offset = if args.len() == 3 {
        match args[2].as_int() {
            Some(n) if n < 0 => return (SIG_OK, Value::NIL),
            Some(n) => n as usize,
            None => return type_error!(ctx, args[2], "bytes/find", "integer for offset"),
        }
    } else {
        0
    };
    let byte = match args[1].as_int() {
        Some(n) if (0..=255).contains(&n) => Some(n as u8),
        Some(n) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("bytes/find: a byte needle must be 0-255, got {}", n),
                ),
            )
        }
        None => None,
    };
    // `Err(())` is a needle of the wrong type, told apart from a haystack of
    // the wrong type by the outer `None`.
    let found = with_byte_content(&args[0], |hay| {
        if offset > hay.len() {
            return Ok(None);
        }
        let rest = &hay[offset..];
        let at = match byte {
            Some(b) => find_bytes(rest, &[b]),
            None => match with_byte_content(&args[1], |needle| find_bytes(rest, needle)) {
                Some(at) => at,
                None => return Err(()),
            },
        };
        Ok(at.map(|p| offset + p))
    });
    match found {
        None => type_error!(ctx, args[0], "bytes/find", "bytes or string for haystack"),
        Some(Err(())) => type_error!(
            ctx,
            args[1],
            "bytes/find",
            "bytes, string or byte for needle"
        ),
        Some(Ok(None)) => (SIG_OK, Value::NIL),
        Some(Ok(Some(at))) => (SIG_OK, Value::int(at as i64)),
    }
}

primitive! {
    "bytes" => prim_bytes {
        ret: RetType::Bytes,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create immutable bytes. Accepts integers (0-255), or a single string/keyword.",
        category: "bytes",
        example: "(bytes 72 101 108 108 111)\n(bytes \"hello\")",
        effect: RegionEffect::Funnel,
    }
    "@bytes" => prim_bytes_mut {
        ret: RetType::MutableBytes,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create mutable bytes. Accepts integers (0-255), or a single string/keyword.",
        category: "bytes",
        example: "(@bytes 72 101 108 108 111)\n(@bytes \"hello\")",
        effect: RegionEffect::Funnel,
    }
    "seq->hex" => prim_seq_to_hex {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert bytes, @bytes, array, @array, list, or integer to a lowercase hex string. Mutable input (@bytes, @array) produces @string; all other input produces string. Integer input uses big-endian minimal-byte encoding (0 → \"00\"). Aliases: bytes->hex, bytes->hex-string.",
        params: &["x"],
        category: "bytes",
        example: "(seq->hex (bytes 72 101 108)) ;=> \"48656c\"\n(seq->hex [72 101 108]) ;=> \"48656c\"\n(seq->hex 255) ;=> \"ff\"",
        aliases: &["bytes->hex", "bytes->hex-string"],
        effect: RegionEffect::Fresh,
    }
    "slice" => prim_slice {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Slice a sequence from start to end index. If end is omitted, slice to end of sequence. Returns same type as input. An immutable string or bytes answers a view over the source's own bytes, with no copy; the source stays alive as long as the slice does.",
        params: &["coll", "start", "end?"],
        category: "bytes",
        example: "(slice [1 2 3 4 5] 1 3)\n(slice \"hello\" 1)",
        effect: RegionEffect::Fresh,
        // Over an immutable string or bytes the fresh header is a view into
        // arg 0's pages, counted by the alloc scan; the embed lets the
        // ownership forest see the result hold that region
        // (docs/impl/region/effects.md).
        embeds: &[0],
    }
    "bytes/find" => prim_bytes_find {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Find the byte index of needle in haystack, from an optional byte offset. Returns the index or nil. The haystack and the needle are bytes or strings, mutable or not; an integer needle is one byte. An empty needle is found at the offset.",
        params: &["haystack", "needle", "offset?"],
        category: "bytes",
        example: "(bytes/find (bytes \"a\\nb\") 10) #=> 1\n(bytes/find \"héllo\" \"l\") #=> 3",
        effect: RegionEffect::Immediate,
    }
}
