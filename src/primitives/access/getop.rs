use super::*;

/// Polymorphic get - works on arrays, @arrays, strings, @strings, and structs
/// `(get collection key [default])`
pub(crate) fn prim_get(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    // Array (mutable indexed collection)
    if let Some(vec_ref) = args[0].as_array_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: array index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let borrowed = vec_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return (SIG_OK, borrowed[i]),
            None => return (SIG_OK, default),
        }
    }

    // Array (immutable indexed collection)
    if let Some(elems) = args[0].as_array() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: array index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        match resolve_index(index, elems.len()) {
            Some(i) => return (SIG_OK, elems[i]),
            None => return (SIG_OK, default),
        }
    }

    // @string (mutable string — indexed by grapheme cluster position)
    if let Some(buf_ref) = args[0].as_string_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: @string index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let borrowed = buf_ref.borrow();
        let s = match std::str::from_utf8(&borrowed) {
            Ok(s) => s,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "encoding-error",
                        format!("get: @string contains invalid UTF-8: {}", e),
                    ),
                )
            }
        };
        if index >= 0 {
            match s.graphemes(true).nth(index as usize) {
                Some(g) => return (SIG_OK, ctx.string(g)),
                None => return (SIG_OK, default),
            }
        } else {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            match resolve_index(index, graphemes.len()) {
                Some(i) => return (SIG_OK, ctx.string(graphemes[i])),
                None => return (SIG_OK, default),
            }
        }
    }

    // Bytes (immutable binary data — indexed by byte position)
    if let Some(b) = args[0].as_bytes() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: bytes index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        match resolve_index(index, b.len()) {
            Some(i) => return (SIG_OK, Value::int(b[i] as i64)),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("get: index {} out of bounds (length {})", index, b.len()),
                    ),
                );
            }
        }
    }

    // @bytes (mutable binary data — indexed by byte position)
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: @bytes index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let borrowed = blob_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return (SIG_OK, Value::int(borrowed[i] as i64)),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!(
                            "get: index {} out of bounds (length {})",
                            index,
                            borrowed.len()
                        ),
                    ),
                );
            }
        }
    }

    // String (immutable grapheme cluster sequence)
    if args[0].is_string() {
        return args[0]
            .with_string(|s| {
                let index = match args[1].as_int() {
                    Some(i) => i,
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error(
                                "type-error",
                                format!(
                                    "get: string index must be integer, got {}",
                                    args[1].type_name()
                                ),
                            ),
                        )
                    }
                };
                if index >= 0 {
                    match s.graphemes(true).nth(index as usize) {
                        Some(g) => (SIG_OK, ctx.string(g)),
                        None => (SIG_OK, default),
                    }
                } else {
                    let graphemes: Vec<&str> = s.graphemes(true).collect();
                    match resolve_index(index, graphemes.len()) {
                        Some(i) => (SIG_OK, ctx.string(graphemes[i])),
                        None => (SIG_OK, default),
                    }
                }
            })
            .unwrap();
    }

    // Struct (mutable keyed collection)
    if let Some(mstruct) = args[0].as_struct_mut() {
        let key = match TableKey::from_value(&args[1]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "struct keys must be immutable (got {})",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let borrowed = mstruct.borrow();
        return (SIG_OK, borrowed.get(&key).copied().unwrap_or(default));
    }

    // Struct (immutable keyed collection)
    if let Some(s) = args[0].as_struct() {
        let key = match TableKey::from_value(&args[1]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "struct keys must be immutable (got {})",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        return (
            SIG_OK,
            sorted_struct_get(s, &key).copied().unwrap_or(default),
        );
    }

    // List (cons-based)
    if args[0].is_pair() || args[0].is_empty_list() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "get: list index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        // Compute list length for negative index resolution
        let resolved = if index >= 0 {
            index as usize
        } else {
            // Walk to compute length
            let mut len = 0usize;
            let mut cur = args[0];
            while let Some(c) = cur.as_pair() {
                len += 1;
                cur = c.rest;
            }
            let r = index + len as i64;
            if r < 0 {
                return (SIG_OK, default);
            }
            r as usize
        };
        let mut current = args[0];
        let mut i = 0usize;
        loop {
            if current.is_empty_list() || current.is_nil() {
                return (SIG_OK, default);
            }
            if let Some(pair) = current.as_pair() {
                if i == resolved {
                    return (SIG_OK, pair.first);
                }
                current = pair.rest;
                i += 1;
            } else {
                return (SIG_OK, default);
            }
        }
    }

    // Unsupported type
    type_error!(
        ctx,
        args[0],
        "get",
        "collection (list, array, @array, string, @string, or struct)"
    )
}
