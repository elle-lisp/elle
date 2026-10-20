// audited: 2026-10-20
//! Read-only seq accessors: first, rest, last, nth, reverse, slice, sort.
//!
//! docs/bytes.md
//! docs/strings.md
//!
//! These never mutate their argument in place (except `sort` of an @array,
//! which is genuinely a query-shaped in-place sort kept here with its
//! immutable siblings). Type-preserving where the operation returns a
//! sequence — an @array in yields an @array out — via the `make_*` builders.

use super::*;
use crate::primitives::access::{resolve_index, resolve_slice_index};

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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |s, _| {
        crate::segment::graphemes(s, gen)
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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |s, m| {
        let rest: String = crate::segment::graphemes(s, gen).skip(1).collect();
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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |s, _| {
        crate::segment::graphemes(s, gen)
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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |s, _| {
        let graphemes: Vec<&str> = crate::segment::graphemes(s, gen).collect();
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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |s, m| {
        let reversed: String = crate::segment::graphemes(s, gen).rev().collect();
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

/// The byte range of graphemes `start..end` of `text`, each index resolved
/// over the grapheme count as `resolve_slice_index` resolves it. Two passes
/// and no allocation: a count, then a walk to the two boundaries.
fn grapheme_byte_range(
    text: &str,
    start: i64,
    end: i64,
    gen: crate::segment::Generation,
) -> (usize, usize) {
    let count = crate::segment::graphemes(text, gen).count();
    let s = resolve_slice_index(start, count);
    let e = resolve_slice_index(end, count);
    if s >= e {
        return (0, 0);
    }
    let (mut from, mut to) = (text.len(), text.len());
    let mut at = 0;
    for (i, g) in crate::segment::graphemes(text, gen).enumerate() {
        if i == s {
            from = at;
        }
        if i == e {
            to = at;
            break;
        }
        at += g.len();
    }
    (from, to)
}

/// Slice a sequence from start to end (type-preserving).
///
/// An immutable string or bytes answers a view over its own payload, and the
/// region walk keeps the source alive under it (docs/impl/values.md). A
/// mutable one copies, because its `Vec` can move under a view.
pub fn seq_slice(val: &Value, start: i64, end: i64, ctx: &mut NativeCtx) -> Result<Value, Value> {
    if let Some(b) = val.as_bytes() {
        let s = resolve_slice_index(start, b.len());
        let e = resolve_slice_index(end, b.len());
        return Ok(ctx.bytes_view(if s >= e { &[] } else { &b[s..e] }));
    }
    if let Some(text) = val.as_str() {
        let (from, to) = grapheme_byte_range(text, start, end, ctx.unicode_generation());
        return Ok(ctx.string_view(&text[from..to]));
    }
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
    let gen = ctx.unicode_generation();
    if let Some(r) = with_text(val, |str_val, m| {
        let graphemes: Vec<&str> = crate::segment::graphemes(str_val, gen).collect();
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

#[cfg(test)]
mod tests;
