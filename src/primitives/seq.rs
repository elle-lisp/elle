//! Seq protocol: centralized dispatch for ordered, indexable sequences.
//!
//! Seq extends Collection — these operations apply only to types with a
//! defined element order: list, (), array, @array, string, @string,
//! bytes, @bytes.  Not sets or structs (unordered).
//!
//! Every operation that may allocate takes the call's `ctx`
//! (docs/impl/region/ctx.md): results are born in the caller's region
//! (Rule 3), through the allocation capability the primitive wrapper hands
//! down — the region is named and passed down explicitly.
use crate::primitives::ctx::NativeCtx;
use crate::value::Value;
use unicode_segmentation::UnicodeSegmentation;

use super::access::{resolve_index, resolve_slice_index};

const SEQ_TYPES: &str = "sequence (list, array, string, bytes)";

fn seq_type_error(op: &str, val: &Value, ctx: &mut NativeCtx) -> Value {
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
fn with_array<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&[Value], bool) -> R,
{
    if let Some(elems) = val.as_array() {
        return Some(f(elems, false));
    }
    if let Some(arr) = val.as_array_mut() {
        let borrowed = arr.borrow();
        return Some(f(&borrowed, true));
    }
    None
}

/// Run `f` over a string's text, whether immutable or @mutable.
/// Returns None for non-string types and for @strings with invalid UTF-8.
fn with_text<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&str, bool) -> R,
{
    // Check immutable string via HeapTag to avoid consuming f in with_string
    if val.is_string() {
        return val.with_string(|s| f(s, false));
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let borrowed = buf_ref.borrow();
        if let Ok(s) = std::str::from_utf8(&borrowed) {
            return Some(f(s, true));
        }
    }
    None
}

/// Run `f` over byte content, whether immutable or @mutable.
fn with_raw_bytes<F, R>(val: &Value, f: F) -> Option<R>
where
    F: FnOnce(&[u8], bool) -> R,
{
    if let Some(b) = val.as_bytes() {
        return Some(f(b, false));
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return Some(f(&borrowed, true));
    }
    None
}

/// Build an array Value, preserving mutability.
fn make_array(elems: Vec<Value>, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.array_mut(elems)
    } else {
        ctx.array(elems)
    }
}

/// Build a string Value, preserving mutability.
fn make_string(s: String, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.string_mut(s.into_bytes())
    } else {
        ctx.string(s)
    }
}

/// Build a bytes Value, preserving mutability.
fn make_bytes(b: Vec<u8>, mutable: bool, ctx: &mut NativeCtx) -> Value {
    if mutable {
        ctx.bytes_mut(b)
    } else {
        ctx.bytes(b)
    }
}

// ── Seq operations ──────────────────────────────────────────────────

/// Get the first element of a sequence.
pub fn seq_first(val: &Value, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if let Some(pair) = val.as_pair() {
        return Ok(pair.first);
    }
    if val.is_empty_list() {
        return Err(ctx.error("argument-error", "first: empty sequence"));
    }
    if let Some(r) = with_array(val, |elems, _| {
        elems
            .first()
            .copied()
            .ok_or_else(|| ctx.error("argument-error", "first: empty sequence"))
    }) {
        return r;
    }
    if let Some(r) = with_text(val, |s, _| {
        s.graphemes(true)
            .next()
            .map(|g| ctx.string(g))
            .ok_or_else(|| ctx.error("argument-error", "first: empty sequence"))
    }) {
        return r;
    }
    if let Some(r) = with_raw_bytes(val, |b, _| {
        if b.is_empty() {
            Err(ctx.error("argument-error", "first: empty sequence"))
        } else {
            Ok(Value::int(b[0] as i64))
        }
    }) {
        return r;
    }
    Err(seq_type_error("first", val, ctx))
}

/// Get the rest of a sequence (type-preserving).
pub fn seq_rest(val: &Value, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if let Some(pair) = val.as_pair() {
        return Ok(pair.rest);
    }
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if let Some(r) = with_array(val, |elems, m| {
        if elems.len() <= 1 {
            make_array(vec![], m, ctx)
        } else {
            make_array(elems[1..].to_vec(), m, ctx)
        }
    }) {
        return Ok(r);
    }
    if let Some(r) = with_text(val, |s, m| {
        let rest: String = s.graphemes(true).skip(1).collect();
        make_string(rest, m, ctx)
    }) {
        return Ok(r);
    }
    if let Some(r) = with_raw_bytes(val, |b, m| {
        if b.len() <= 1 {
            make_bytes(vec![], m, ctx)
        } else {
            make_bytes(b[1..].to_vec(), m, ctx)
        }
    }) {
        return Ok(r);
    }
    Err(seq_type_error("rest", val, ctx))
}

/// Get the last element of a sequence.
pub fn seq_last(val: &Value, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if val.is_empty_list() {
        return Err(ctx.error("argument-error", "last: empty sequence"));
    }
    if val.is_pair() {
        let mut current = *val;
        let mut last = Value::NIL;
        while let Some(pair) = current.as_pair() {
            last = pair.first;
            current = pair.rest;
        }
        return Ok(last);
    }
    if let Some(r) = with_array(val, |elems, _| {
        elems
            .last()
            .copied()
            .ok_or_else(|| ctx.error("argument-error", "last: empty sequence"))
    }) {
        return r;
    }
    if let Some(r) = with_text(val, |s, _| {
        s.graphemes(true)
            .next_back()
            .map(|g| ctx.string(g))
            .ok_or_else(|| ctx.error("argument-error", "last: empty sequence"))
    }) {
        return r;
    }
    if let Some(r) = with_raw_bytes(val, |b, _| {
        b.last()
            .map(|&byte| Value::int(byte as i64))
            .ok_or_else(|| ctx.error("argument-error", "last: empty sequence"))
    }) {
        return r;
    }
    Err(seq_type_error("last", val, ctx))
}

/// Get element at index n.
pub fn seq_nth(val: &Value, n: i64, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if val.is_pair() {
        if n >= 0 {
            let mut current = *val;
            let mut i = 0usize;
            loop {
                if current.is_empty_list() || current.is_nil() {
                    return Err(
                        ctx.error("argument-error", format!("nth: index {} out of bounds", n))
                    );
                }
                if let Some(p) = current.as_pair() {
                    if i == n as usize {
                        return Ok(p.first);
                    }
                    current = p.rest;
                    i += 1;
                } else {
                    return Err(
                        ctx.error("argument-error", format!("nth: index {} out of bounds", n))
                    );
                }
            }
        } else {
            let mut len = 0usize;
            let mut cur = *val;
            while let Some(c) = cur.as_pair() {
                len += 1;
                cur = c.rest;
            }
            let resolved = n + len as i64;
            if resolved < 0 {
                return Err(ctx.error(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, len),
                ));
            }
            return seq_nth(val, resolved, ctx);
        }
    }
    if val.is_empty_list() {
        return Err(ctx.error(
            "argument-error",
            format!("nth: index {} out of bounds (empty list)", n),
        ));
    }
    if let Some(r) = with_array(val, |elems, _| {
        resolve_index(n, elems.len())
            .map(|i| elems[i])
            .ok_or_else(|| {
                ctx.error(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, elems.len()),
                )
            })
    }) {
        return r;
    }
    if let Some(r) = with_text(val, |s, _| {
        let graphemes: Vec<&str> = s.graphemes(true).collect();
        resolve_index(n, graphemes.len())
            .map(|i| ctx.string(graphemes[i]))
            .ok_or_else(|| {
                ctx.error(
                    "argument-error",
                    format!(
                        "nth: index {} out of bounds (length {})",
                        n,
                        graphemes.len()
                    ),
                )
            })
    }) {
        return r;
    }
    if let Some(r) = with_raw_bytes(val, |b, _| {
        resolve_index(n, b.len())
            .map(|i| Value::int(b[i] as i64))
            .ok_or_else(|| {
                ctx.error(
                    "argument-error",
                    format!("nth: index {} out of bounds (length {})", n, b.len()),
                )
            })
    }) {
        return r;
    }
    Err(seq_type_error("nth", val, ctx))
}

/// Reverse a sequence (type-preserving).
pub fn seq_reverse(val: &Value, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if val.is_pair() {
        let mut vec = val
            .list_to_vec()
            .map_err(|e| ctx.error("type-error", e.to_string()))?;
        vec.reverse();
        return Ok(ctx.list(vec));
    }
    if let Some(r) = with_array(val, |elems, m| {
        let mut vec = elems.to_vec();
        vec.reverse();
        make_array(vec, m, ctx)
    }) {
        return Ok(r);
    }
    if let Some(r) = with_text(val, |s, m| {
        let reversed: String = s.graphemes(true).rev().collect();
        make_string(reversed, m, ctx)
    }) {
        return Ok(r);
    }
    if let Some(r) = with_raw_bytes(val, |b, m| {
        let mut vec = b.to_vec();
        vec.reverse();
        make_bytes(vec, m, ctx)
    }) {
        return Ok(r);
    }
    Err(seq_type_error("reverse", val, ctx))
}

/// Slice a sequence from start to end (type-preserving).
pub fn seq_slice(val: &Value, start: i64, end: i64, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if let Some(r) = with_raw_bytes(val, |b, m| {
        let s = resolve_slice_index(start, b.len());
        let e = resolve_slice_index(end, b.len());
        if s >= e {
            make_bytes(vec![], m, ctx)
        } else {
            make_bytes(b[s..e].to_vec(), m, ctx)
        }
    }) {
        return Ok(r);
    }
    if let Some(r) = with_array(val, |elems, m| {
        let s = resolve_slice_index(start, elems.len());
        let e = resolve_slice_index(end, elems.len());
        if s >= e {
            make_array(vec![], m, ctx)
        } else {
            make_array(elems[s..e].to_vec(), m, ctx)
        }
    }) {
        return Ok(r);
    }
    if let Some(r) = with_text(val, |str_val, m| {
        let graphemes: Vec<&str> = str_val.graphemes(true).collect();
        let s = resolve_slice_index(start, graphemes.len()).min(graphemes.len());
        let e = resolve_slice_index(end, graphemes.len()).min(graphemes.len());
        if s >= e {
            make_string(String::new(), m, ctx)
        } else {
            make_string(graphemes[s..e].concat(), m, ctx)
        }
    }) {
        return Ok(r);
    }
    // Lists
    if val.is_empty_list() || val.is_pair() {
        let elems = val
            .list_to_vec()
            .map_err(|e| ctx.error("type-error", e.to_string()))?;
        let s = resolve_slice_index(start, elems.len());
        let e = resolve_slice_index(end, elems.len());
        if s >= e {
            return Ok(Value::EMPTY_LIST);
        }
        let mut result = Value::EMPTY_LIST;
        for v in elems[s..e].iter().rev() {
            result = ctx.pair(*v, result);
        }
        return Ok(result);
    }
    Err(seq_type_error("slice", val, ctx))
}

/// Sort a sequence (type-preserving).
pub fn seq_sort(val: &Value, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if val.is_array_mut() {
        crate::value::arena::with_array_mut_neutral(*val, |vec| vec.sort());
        return Ok(*val);
    }
    if let Some(elems) = val.as_array() {
        let mut vec = elems.to_vec();
        vec.sort();
        return Ok(ctx.array(vec));
    }
    if val.is_empty_list() {
        return Ok(Value::EMPTY_LIST);
    }
    if val.is_pair() {
        let mut vec = val
            .list_to_vec()
            .map_err(|e| ctx.error("type-error", e.to_string()))?;
        vec.sort();
        return Ok(ctx.list(vec));
    }
    Err(ctx.error(
        "type-error",
        format!(
            "sort: expected list, array, or @array, got {}",
            val.type_name()
        ),
    ))
}

/// Push an element onto the end of a sequence (type-aware).
pub fn seq_push(
    val: &Value,
    elem: Value,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Result<Value, Value> {
    // @array — mutate in place
    if val.is_array_mut() {
        return Ok(crate::value::arena::push_with_incref(
            ctx.heap_mut(),
            *val,
            elem,
        ));
    }
    // @string — append string
    if let Some(buf_ref) = val.as_string_mut() {
        let s = elem.with_string(|s| s.to_string()).ok_or_else(|| {
            ctx.error(
                "type-error",
                format!(
                    "push: @string value must be string, got {}",
                    elem.type_name()
                ),
            )
        })?;
        buf_ref.borrow_mut().extend_from_slice(s.as_bytes());
        return Ok(*val);
    }
    // @bytes — append byte
    if let Some(blob_ref) = val.as_bytes_mut() {
        let byte = require_byte("push", &elem, ctx)?;
        blob_ref.borrow_mut().push(byte);
        return Ok(*val);
    }
    // Immutable array
    if let Some(elems) = val.as_array() {
        let mut new = elems.to_vec();
        new.push(elem);
        return Ok(ctx.array(new));
    }
    // Immutable string
    if val.is_string() {
        let s = elem.with_string(|s| s.to_string()).ok_or_else(|| {
            ctx.error(
                "type-error",
                format!(
                    "push: string value must be string, got {}",
                    elem.type_name()
                ),
            )
        })?;
        return val
            .with_string(|base| {
                let mut new = base.to_string();
                new.push_str(&s);
                Ok(ctx.string(new))
            })
            .unwrap();
    }
    // Immutable bytes
    if let Some(b) = val.as_bytes() {
        let byte = require_byte("push", &elem, ctx)?;
        let mut new = b.to_vec();
        new.push(byte);
        return Ok(ctx.bytes(new));
    }
    Err(ctx.error(
        "type-error",
        format!(
            "push: expected array, @array, string, @string, bytes, or @bytes, got {}",
            val.type_name()
        ),
    ))
}

/// Pop the last element from a mutable sequence.
pub fn seq_pop(val: &Value, ctx: &mut crate::primitives::ctx::Alloc) -> Result<Value, Value> {
    if val.is_array_mut() {
        if val.array_mut_ref().unwrap().is_empty() {
            return Err(ctx.error("argument-error", "pop: empty @array"));
        }
        return Ok(crate::value::arena::pop_with_decref(ctx.heap_mut(), *val));
    }
    if let Some(buf_ref) = val.as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        if buf.is_empty() {
            return Err(ctx.error("argument-error", "pop: empty @string"));
        }
        let s = std::str::from_utf8(&buf)
            .map_err(|_| ctx.error("encoding-error", "pop: @string contains invalid UTF-8"))?;
        let cluster = s.graphemes(true).next_back().unwrap().to_string();
        let new_len = buf.len() - cluster.len();
        buf.truncate(new_len);
        drop(buf);
        return Ok(ctx.string(cluster));
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        let mut blob = blob_ref.borrow_mut();
        match blob.pop() {
            Some(byte) => {
                drop(blob);
                return Ok(Value::int(byte as i64));
            }
            None => return Err(ctx.error("argument-error", "pop: empty @bytes")),
        }
    }
    Err(ctx.error(
        "type-error",
        format!(
            "pop: expected @array, @string, or @bytes, got {}",
            val.type_name()
        ),
    ))
}

/// Validate and extract a byte value from an integer.
fn require_byte(
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
