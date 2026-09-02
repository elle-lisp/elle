use super::*;

/// Convert to string (variadic: 0 args → "", 1 arg → convert, N args → concatenate)
pub(crate) fn prim_to_string(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args.len() {
        0 => (SIG_OK, ctx.string("")),
        1 => prim_to_string_single(ctx, args[0]),
        _ => {
            // Multi-arg: format directly into a Rust String to avoid
            // allocating slab-backed intermediate strings per argument.
            let mut result = String::new();
            for arg in args {
                if let Err((sig, val)) = write_value_to_string(ctx, *arg, &mut result) {
                    return (sig, val);
                }
            }
            (SIG_OK, ctx.string(result))
        }
    }
}

/// Append a value's string representation directly to a Rust String,
/// avoiding slab allocation for intermediates.
fn write_value_to_string(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    val: Value,
    out: &mut String,
) -> Result<(), (SignalBits, Value)> {
    use std::fmt::Write;

    if val.is_string() {
        val.with_string(|s| out.push_str(s));
        return Ok(());
    }
    if let Some(ms) = val.as_string_mut() {
        let borrowed = ms.borrow();
        match std::str::from_utf8(&borrowed) {
            Ok(s) => out.push_str(s),
            Err(e) => {
                return Err((
                    SIG_ERROR,
                    ctx.error("encoding-error", format!("string: invalid UTF-8: {}", e)),
                ))
            }
        }
        return Ok(());
    }
    if let Some(n) = val.as_int() {
        let _ = write!(out, "{}", n);
        return Ok(());
    }
    if let Some(f) = val.as_float() {
        if f.is_infinite() {
            out.push_str(if f.is_sign_positive() { "inf" } else { "-inf" });
        } else if f.is_nan() {
            out.push_str("NaN");
        } else if f.fract() == 0.0 {
            let _ = write!(out, "{:.1}", f);
        } else {
            let _ = write!(out, "{}", f);
        }
        return Ok(());
    }
    if let Some(b) = val.as_bool() {
        out.push_str(if b { "true" } else { "false" });
        return Ok(());
    }
    if val.is_nil() {
        out.push_str("nil");
        return Ok(());
    }
    if let Some(name) = ctx.keyword_spelling(val) {
        out.push_str(&name);
        return Ok(());
    }
    if let Some(sym_id) = val.as_symbol() {
        // Copied out of the memo before `ctx` is touched again: `symbols()`
        // hands back a borrow into the instance's map, and `ctx.error` below
        // reborrows the same ctx.
        let name = ctx
            .vm()
            .symbols()
            .and_then(|s| s.name(sym_id))
            .map(str::to_string);
        match name {
            Some(name) => out.push_str(&name),
            None => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
                        "internal-error",
                        format!("to-string: symbol ID {} has no recorded name", sym_id),
                    ),
                ))
            }
        }
        return Ok(());
    }
    // For compound/heap types, fall back to prim_to_string_single
    // (these are rare in hot concat paths).
    let (sig, string_val) = prim_to_string_single(ctx, val);
    if sig != SIG_OK {
        return Err((sig, string_val));
    }
    if let Some(s) = string_val.with_string(|s| s.to_string()) {
        out.push_str(&s);
    } else {
        return Err((
            SIG_ERROR,
            ctx.error(
                "internal-error",
                "to-string: internal conversion failure".to_string(),
            ),
        ));
    }
    Ok(())
}

/// Single-value string conversion (original behavior).
fn prim_to_string_single(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    val: Value,
) -> (SignalBits, Value) {
    // Already a string: return a FRESH copy in the call's own region. `string` is
    // declared `RegionEffect::Fresh`, whose declaration oracle requires the result to
    // live in this call's region; passing `val` through (it lives in the caller's
    // region) would both trip that oracle and — since `Fresh` marks no argument
    // escaping — is unnecessary. The copy keeps every `string` path fresh, which is
    // what removes the per-heap-arg leak the old `Mixed` declaration caused
    // (tests/elle/region-string-concat-leak.lisp).
    if let Some(s) = val.with_string(|s| s.to_string()) {
        return (SIG_OK, ctx.string(s));
    }

    // @string: convert to immutable string
    if let Some(ms) = val.as_string_mut() {
        let borrowed = ms.borrow();
        return match std::str::from_utf8(&borrowed) {
            Ok(s) => (SIG_OK, ctx.string(s)),
            Err(e) => (
                SIG_ERROR,
                ctx.error("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    // bytes (immutable): UTF-8 decode to immutable string
    if let Some(b) = val.as_bytes() {
        return match std::str::from_utf8(b) {
            Ok(s) => (SIG_OK, ctx.string(s)),
            Err(e) => (
                SIG_ERROR,
                ctx.error("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    // @bytes (mutable): UTF-8 decode to immutable string
    if let Some(blob_ref) = val.as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return match std::str::from_utf8(&borrowed) {
            Ok(s) => (SIG_OK, ctx.string(s)),
            Err(e) => (
                SIG_ERROR,
                ctx.error("encoding-error", format!("string: invalid UTF-8: {}", e)),
            ),
        };
    }

    if let Some(n) = val.as_int() {
        return (SIG_OK, ctx.string(n.to_string()));
    }

    if let Some(f) = val.as_float() {
        let s = if f.is_infinite() {
            if f.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else if f.is_nan() {
            "NaN".to_string()
        } else if f.fract() == 0.0 {
            format!("{:.1}", f)
        } else {
            f.to_string()
        };
        return (SIG_OK, ctx.string(s));
    }

    if let Some(b) = val.as_bool() {
        return (SIG_OK, ctx.string(if b { "true" } else { "false" }));
    }

    if val.is_nil() {
        return (SIG_OK, ctx.string("nil"));
    }

    if let Some(sym_id) = val.as_symbol() {
        let name = ctx
            .vm()
            .symbols()
            .and_then(|s| s.name(sym_id))
            .map(str::to_string);
        return match name {
            Some(name) => (SIG_OK, ctx.string(&name)),
            None => (
                SIG_ERROR,
                ctx.error(
                    "internal-error",
                    format!("to-string: symbol ID {} has no recorded name", sym_id),
                ),
            ),
        };
    }

    if let Some(name) = ctx.keyword_spelling(val) {
        return (SIG_OK, ctx.string(name));
    }

    // Handle heap types (Pair, Array, etc.)
    if let Some(_cons) = val.as_pair() {
        let mut items = Vec::new();
        let mut current = val;
        loop {
            if current.is_nil() || current.is_empty_list() {
                break;
            }
            if let Some(c) = current.as_pair() {
                items.push(c.first);
                current = c.rest;
            } else {
                items.push(current);
                break;
            }
        }

        let mut formatted_items = Vec::new();
        for v in items {
            let (sig, result) = prim_to_string_single(ctx, v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "internal-error",
                        "to-string: failed to convert list item".to_string(),
                    ),
                );
            }
        }

        let list_str = format!("({})", formatted_items.join(" "));
        return (SIG_OK, ctx.string(list_str));
    }

    if let Some(vec_ref) = val.as_array_mut() {
        let vec = vec_ref.borrow();
        let mut formatted_items = Vec::new();
        for v in vec.iter() {
            let (sig, result) = prim_to_string_single(ctx, *v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "internal-error",
                        "to-string: failed to convert array item".to_string(),
                    ),
                );
            }
        }

        let vec_str = format!("@[{}]", formatted_items.join(" "));
        return (SIG_OK, ctx.string(vec_str));
    }

    if let Some(elems) = val.as_array() {
        let mut formatted_items = Vec::new();
        for v in elems.iter() {
            let (sig, result) = prim_to_string_single(ctx, *v);
            if sig != SIG_OK {
                return (sig, result);
            }
            if let Some(s) = result.with_string(|s| s.to_string()) {
                formatted_items.push(s);
            } else {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "internal-error",
                        "to-string: failed to convert array item".to_string(),
                    ),
                );
            }
        }

        let vec_str = format!("[{}]", formatted_items.join(" "));
        return (SIG_OK, ctx.string(vec_str));
    }

    // For other types, render Debug-style through the instance memo, so
    // nested symbol and keyword spellings resolve (docs/impl/symbol.md
    // § "Reading a name, and not reading one").
    let repr = {
        let symbols = unsafe { ctx.vm().symbols_ptr.as_ref() };
        format!("{}", val.debug_with(symbols))
    };
    (SIG_OK, ctx.string(repr))
}
