use super::*;

/// Split string or @string on delimiter, returning an array
pub(crate) fn prim_string_split(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, _is_buffer) = match as_text(&args[0], "string-split", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let delimiter = if let Some(d) = args[1].with_string(|s| s.to_string()) {
        d
    } else {
        return type_error!(ctx, args[1], "string-split", "string");
    };

    if delimiter.is_empty() {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "string-split: delimiter cannot be empty".to_string(),
            ),
        );
    }

    let parts: Vec<Value> = s.split(&delimiter).map(|s| ctx.string(s)).collect();

    (SIG_OK, ctx.array(parts))
}

/// Replace all occurrences of old with new in a string or buffer
pub(crate) fn prim_string_replace(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, is_buffer) = match as_text(&args[0], "string-replace", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let old = if let Some(o) = args[1].with_string(|s| s.to_string()) {
        o
    } else {
        return type_error!(ctx, args[1], "string-replace", "string");
    };

    if old.is_empty() {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "string-replace: search string cannot be empty".to_string(),
            ),
        );
    }

    let new = if let Some(n) = args[2].with_string(|s| s.to_string()) {
        n
    } else {
        return type_error!(ctx, args[2], "string-replace", "string");
    };

    let replaced = s.replace(&*old, &new);
    if is_buffer {
        (SIG_OK, ctx.string_mut(replaced.into_bytes()))
    } else {
        (SIG_OK, ctx.string(replaced))
    }
}

/// Trim leading and trailing whitespace from a string or buffer
pub(crate) fn prim_string_trim(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, is_buffer) = match as_text(&args[0], "string-trim", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let trimmed = s.trim().to_string();
    if is_buffer {
        (SIG_OK, ctx.string_mut(trimmed.into_bytes()))
    } else {
        (SIG_OK, ctx.string(trimmed))
    }
}

/// Check if string or buffer contains substring
pub(crate) fn prim_string_contains(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (haystack, _is_buffer) = match as_text(&args[0], "string-contains?", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let needle = if let Some(n) = args[1].with_string(|s| s.to_string()) {
        n
    } else {
        return type_error!(ctx, args[1], "string-contains?", "string");
    };

    (
        SIG_OK,
        if haystack.contains(&*needle) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string or buffer starts with prefix
pub(crate) fn prim_string_starts_with(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, _is_buffer) = match as_text(&args[0], "string-starts-with?", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let prefix = if let Some(p) = args[1].with_string(|s| s.to_string()) {
        p
    } else {
        return type_error!(ctx, args[1], "string-starts-with?", "string");
    };

    (
        SIG_OK,
        if s.starts_with(&*prefix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Check if string or buffer ends with suffix
pub(crate) fn prim_string_ends_with(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (s, _is_buffer) = match as_text(&args[0], "string-ends-with?", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let suffix = if let Some(suf) = args[1].with_string(|s| s.to_string()) {
        suf
    } else {
        return type_error!(ctx, args[1], "string-ends-with?", "string");
    };

    (
        SIG_OK,
        if s.ends_with(&*suffix) {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}

/// Join sequence of strings with separator
pub(crate) fn prim_string_join(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let seq = &args[0];
    let separator = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[1], "string-join", "string");
    };

    // Try tuple first
    let vec = if let Some(elems) = seq.as_array() {
        elems.to_vec()
    } else if let Some(arr) = seq.as_array_mut() {
        arr.borrow().clone()
    } else {
        // Fall back to list unwrapping for lists and syntax
        match seq.list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(_) => {
                return type_error!(ctx, seq, "string-join", "sequence (list, tuple, or array)")
            }
        }
    };

    let mut strings = Vec::new();

    for val in vec {
        match val.with_string(|s| s.to_string()) {
            Some(s) => strings.push(s),
            None => return type_error!(ctx, val, "string-join", "string"),
        }
    }

    (SIG_OK, ctx.string(strings.join(&separator)))
}
