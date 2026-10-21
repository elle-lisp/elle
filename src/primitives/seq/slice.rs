// audited: 2026-10-20
//! The seq `slice`: a view over an immutable string or bytes, and a copy of every other sequence.
//!
//! docs/bytes.md
//! docs/strings.md

use super::*;
use crate::primitives::access::resolve_slice_index;

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

#[cfg(test)]
mod tests;
