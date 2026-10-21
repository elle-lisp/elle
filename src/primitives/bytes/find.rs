// audited: 2026-10-20
//! The `bytes/find` primitive: the byte index of a needle in any of the four byte-carrying types.
//!
//! docs/bytes.md

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

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
