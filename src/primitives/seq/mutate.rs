//! In-place seq mutation: push and pop.
//!
//! Unlike the query ops these take an `Alloc` (not the fuller `NativeCtx`),
//! since growing/shrinking a @mutable container needs heap refcount
//! maintenance but no signal machinery. Immutable inputs fall back to
//! allocating a fresh copy so the surface API stays total.

use super::*;

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

/// Pop the last element from a mutable sequence. `gen` decides the
/// grapheme boundaries of an @string's final cluster.
pub fn seq_pop(
    val: &Value,
    gen: crate::segment::Generation,
    ctx: &mut crate::primitives::ctx::Alloc,
) -> Result<Value, Value> {
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
        let cluster = crate::segment::graphemes(s, gen)
            .next_back()
            .unwrap()
            .to_string();
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
