use super::*;

/// Dispatch a call on a collection value.
///
/// Returns `None` if the value is not a callable collection.
/// Returns `Some(Ok(value))` on success.
/// Returns `Some(Err((kind, msg)))` on error.
pub(crate) fn call_collection(
    func: &Value,
    args: &[Value],
    ctx: &mut crate::primitives::ctx::Alloc<'_>,
) -> Option<Result<Value, (&'static str, String)>> {
    // ── Structs (immutable and mutable) ──────────────────────────────
    if let Some(s) = func.as_struct() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("struct call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let key = match TableKey::from_value(&args[0]) {
            Some(k) => k,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "struct call: struct keys must be immutable (got {})",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return Some(Ok(sorted_struct_get(s, &key).copied().unwrap_or(default)));
    }
    if let Some(s) = func.as_struct_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@struct call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let key = match TableKey::from_value(&args[0]) {
            Some(k) => k,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@struct call: struct keys must be immutable (got {})",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return Some(Ok(s.borrow().get(&key).copied().unwrap_or(default)));
    }

    // ── Arrays (immutable and mutable) ───────────────────────────────
    if let Some(elems) = func.as_array() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("array call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "array call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        match resolve_index(index, elems.len()) {
            Some(i) => return Some(Ok(elems[i])),
            None => return Some(Ok(default)),
        }
    }
    if let Some(vec_ref) = func.as_array_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@array call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@array call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = vec_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return Some(Ok(borrowed[i])),
            None => return Some(Ok(default)),
        }
    }

    // ── Strings (immutable and mutable) ────────────────────────────────
    if func.is_string() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("string call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "string call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return func
            .with_string(|s| {
                use unicode_segmentation::UnicodeSegmentation;
                if index >= 0 {
                    match s.graphemes(true).nth(index as usize) {
                        Some(g) => Some(Ok(ctx.string(g))),
                        None => Some(Ok(default)),
                    }
                } else {
                    let graphemes: Vec<&str> = s.graphemes(true).collect();
                    match resolve_index(index, graphemes.len()) {
                        Some(i) => Some(Ok(ctx.string(graphemes[i]))),
                        None => Some(Ok(default)),
                    }
                }
            })
            .unwrap();
    }
    if let Some(buf_ref) = func.as_string_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@string call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@string call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = buf_ref.borrow();
        let s = match std::str::from_utf8(&borrowed) {
            Ok(s) => s,
            Err(e) => {
                return Some(Err((
                    "encoding-error",
                    format!("@string call: invalid UTF-8: {}", e),
                )))
            }
        };
        use unicode_segmentation::UnicodeSegmentation;
        if index >= 0 {
            return match s.graphemes(true).nth(index as usize) {
                Some(g) => Some(Ok(ctx.string(g))),
                None => Some(Ok(default)),
            };
        } else {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            return match resolve_index(index, graphemes.len()) {
                Some(i) => Some(Ok(ctx.string(graphemes[i]))),
                None => Some(Ok(default)),
            };
        }
    }

    // ── Bytes (immutable and mutable) ────────────────────────────────
    if let Some(b) = func.as_bytes() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("bytes call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "bytes call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        match resolve_index(index, b.len()) {
            Some(i) => return Some(Ok(Value::int(b[i] as i64))),
            None => return Some(Ok(default)),
        }
    }
    if let Some(blob_ref) = func.as_bytes_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@bytes call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@bytes call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = blob_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return Some(Ok(Value::int(borrowed[i] as i64))),
            None => return Some(Ok(default)),
        }
    }

    // ── Sets (immutable and mutable) ─────────────────────────────────
    if let Some(s) = func.as_set() {
        if args.len() != 1 {
            return Some(Err((
                "arity-error",
                format!("set call: expected 1 argument, got {}", args.len()),
            )));
        }
        let frozen = crate::primitives::sets::freeze_value(args[0], ctx);
        return Some(Ok(Value::bool(s.contains(&frozen))));
    }
    if let Some(s) = func.as_set_mut() {
        if args.len() != 1 {
            return Some(Err((
                "arity-error",
                format!("@set call: expected 1 argument, got {}", args.len()),
            )));
        }
        let frozen = crate::primitives::sets::freeze_value(args[0], ctx);
        return Some(Ok(Value::bool(s.borrow().contains(&frozen))));
    }

    None
}
