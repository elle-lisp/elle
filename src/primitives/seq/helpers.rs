// audited: 2026-10-22
//! Container dispatch and result-building helpers shared by seq operations.
//!
//! docs/impl/region/ctx.md
//!
//! Seq ops apply uniformly to arrays, strings, and bytes in both immutable and
//! @mutable forms. Each 2-way (immut vs @mut) branch collapses into a single
//! `with_*` call, and each result into a `make_*` builder that preserves
//! mutability. The per-operation code then holds the operation itself, not a
//! re-derivation of the container taxonomy.

use super::*;

pub(super) const SEQ_TYPES: &str = "sequence (list, array, string, bytes)";

pub(super) fn seq_type_error(op: &str, val: &Value, ctx: &mut NativeCtx) -> Value {
    ctx.error(
        "type-error",
        format!("{}: expected {}, got {}", op, SEQ_TYPES, val.type_name()),
    )
}

// ── Mutable/immutable dispatch helpers ──────────────────────────────
//
// These collapse the 2-way branch (immut vs @mut) into a single call
// for each container family.  The `mutable` flag lets callers preserve
// mutability in the result when needed. They take a closure that captures
// the call's `ctx`, so they need no ctx parameter themselves.

/// Run `f` over an array's elements, whether immutable or @mutable.
pub(super) fn with_array<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&[Value], bool) -> R,
{
    if let Some(elems) = val.as_array() {
        return Some(f(elems, false));
    }
    if let Some(elems) = val.array_mut_read() {
        return Some(f(&elems, true));
    }
    None
}

/// Run `f` over a string's text, whether immutable or @mutable.
/// Returns None for non-string types and for @strings with invalid UTF-8.
pub(super) fn with_text<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&str, bool) -> R,
{
    // Check immutable string via HeapTag to avoid consuming f in with_string
    if val.is_string() {
        return val.with_string(|s| f(s, false));
    }
    if let Some(bytes) = val.string_mut_read() {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            return Some(f(s, true));
        }
    }
    None
}

/// Run `f` over byte content, whether immutable or @mutable.
pub(super) fn with_raw_bytes<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&[u8], bool) -> R,
{
    if let Some(b) = val.as_bytes() {
        return Some(f(b, false));
    }
    if let Some(bytes) = val.bytes_mut_read() {
        return Some(f(&bytes, true));
    }
    None
}

/// Build an array Value, preserving mutability.
pub(super) fn make_array(elems: Vec<Value>, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.array_mut(elems)
    } else {
        ctx.array(elems)
    }
}

/// Build a string Value, preserving mutability.
pub(super) fn make_string(s: String, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.string_mut(s.into_bytes())
    } else {
        ctx.string(s)
    }
}

/// Build a bytes Value, preserving mutability.
pub(super) fn make_bytes(b: Vec<u8>, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.bytes_mut(b)
    } else {
        ctx.bytes(b)
    }
}

/// Validate and extract a byte value from an integer.
pub(super) fn require_byte(
    op: &str,
    val: &Value,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Result<u8, Value> {
    match val.as_int() {
        Some(n) if (0..=255).contains(&n) => Ok(n as u8),
        Some(n) => Err(ctx.error(
            "argument-error",
            format!("{}: byte value out of range 0-255: {}", op, n),
        )),
        None => Err(ctx.error(
            "type-error",
            format!(
                "{}: bytes value must be integer, got {}",
                op,
                val.type_name()
            ),
        )),
    }
}
