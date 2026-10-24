// audited: 2026-10-22
//! Polymorphic collection access: `get` reads by index or key, and `put` writes one.
//!
//! docs/structs.md
//!
//! - `get` reads arrays, @arrays, strings, @strings, bytes, @bytes, lists, and
//!   structs.
//! - `put` writes @arrays, arrays, strings, @strings, @bytes, and structs.

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{sorted_struct_get, sorted_struct_insert, TableKey, Value};

mod getop;
pub(crate) use getop::*;

/// Resolve a possibly-negative index. Returns None if out of bounds.
pub(crate) fn resolve_index(index: i64, len: usize) -> Option<usize> {
    if index >= 0 {
        let i = index as usize;
        if i >= len {
            None
        } else {
            Some(i)
        }
    } else {
        let r = index + len as i64;
        if r < 0 {
            None
        } else {
            Some(r as usize)
        }
    }
}

/// Resolve a possibly-negative slice bound, clamping to [0, len].
pub(crate) fn resolve_slice_index(index: i64, len: usize) -> usize {
    if index >= 0 {
        (index as usize).min(len)
    } else {
        let r = index + len as i64;
        if r < 0 {
            0
        } else {
            r as usize
        }
    }
}

/// Polymorphic put: `(put collection key value)`.
///
/// - An @array, @string or @bytes mutates in place and returns itself.
/// - An array or string returns a new value with the element replaced.
/// - An @struct mutates in place; a struct returns a new struct.
pub(crate) fn prim_put(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // 2-arg form: (put set value) — delegates to add
    if args.len() == 2 {
        if args[0].is_set() || args[0].is_set_mut() {
            return crate::primitives::sets::prim_add(ctx, args);
        }
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
                format!(
                    "put: 2-argument form requires a set, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    // @string (mutable string — indexed by grapheme cluster position)
    if let Some(bytes) = args[0].string_mut_read() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: @string index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let replacement = match args[2].with_string(|r| r.to_string()) {
            Some(r) => r,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: @string value must be string, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let s = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "encoding-error",
                        format!("put: @string contains invalid UTF-8: {}", e),
                    ),
                )
            }
        };
        let graphemes: Vec<&str> = crate::segment::graphemes(s, ctx.unicode_generation()).collect();
        let resolved = match resolve_index(index, graphemes.len()) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!(
                            "put: index {} out of bounds (length {})",
                            index,
                            graphemes.len()
                        ),
                    ),
                );
            }
        };
        let start: usize = graphemes[..resolved].iter().map(|g| g.len()).sum();
        let span = start..start + graphemes[resolved].len();
        // The splice refuses to run under a live read.
        drop(bytes);
        crate::value::arena::bytes_splice(ctx.heap_mut(), args[0], span, replacement.as_bytes());
        return (SIG_OK, args[0]);
    }

    // @bytes (mutable byte sequence) - mutate in place
    if let Some(len) = args[0].bytes_mut_read().map(|b| b.len()) {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: @bytes index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let byte = match args[2].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("put: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: @bytes value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let resolved = match resolve_index(index, len) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("put: index {} out of bounds (length {})", index, len),
                    ),
                );
            }
        };
        crate::value::arena::bytes_splice(ctx.heap_mut(), args[0], resolved..resolved + 1, &[byte]);
        return (SIG_OK, args[0]);
    }

    // Array (mutable indexed collection) - mutate in place
    if args[0].is_array_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: array index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let len = args[0].array_mut_read().map_or(0, |a| a.len());
        let resolved = match resolve_index(index, len) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("put: index {} out of bounds (length {})", index, len),
                    ),
                );
            }
        };
        crate::value::arena::set_at_with_rebind(ctx.heap_mut(), args[0], resolved, args[2]);
        return (SIG_OK, args[0]); // Return the mutated array
    }

    // Array (immutable indexed collection) - return new array
    if let Some(elems) = args[0].as_array() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "put: array index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let resolved = match resolve_index(index, elems.len()) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!(
                            "put: index {} out of bounds (length {})",
                            index,
                            elems.len()
                        ),
                    ),
                );
            }
        };
        let mut new_elems = elems.to_vec();
        new_elems[resolved] = args[2];
        return (SIG_OK, ctx.array(new_elems));
    }

    // String (immutable grapheme cluster sequence) - return new string
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
                                    "put: string index must be integer, got {}",
                                    args[1].type_name()
                                ),
                            ),
                        )
                    }
                };
                let replacement = match args[2].with_string(|r| r.to_string()) {
                    Some(r) => r,
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error(
                                "type-error",
                                format!(
                                    "put: string value must be string, got {}",
                                    args[2].type_name()
                                ),
                            ),
                        )
                    }
                };
                let graphemes: Vec<&str> =
                    crate::segment::graphemes(s, ctx.unicode_generation()).collect();
                let resolved = match resolve_index(index, graphemes.len()) {
                    Some(i) => i,
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error(
                                "argument-error",
                                format!(
                                    "put: index {} out of bounds (length {})",
                                    index,
                                    graphemes.len()
                                ),
                            ),
                        );
                    }
                };
                let mut result = String::new();
                for (i, g) in graphemes.iter().enumerate() {
                    if i == resolved {
                        result.push_str(&replacement);
                    } else {
                        result.push_str(g);
                    }
                }
                (SIG_OK, ctx.string(result.as_str()))
            })
            .unwrap();
    }

    // Struct (mutable keyed collection) - mutate in place
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
    let value = args[2];

    if args[0].is_struct_mut() {
        crate::value::arena::struct_put_with_rebind(ctx.heap_mut(), args[0], key, value);
        return (SIG_OK, args[0]); // Return the mutated struct
    }

    // Struct (immutable keyed collection) - return new struct
    if let Some(s) = args[0].as_struct() {
        return (
            SIG_OK,
            ctx.struct_from_sorted(sorted_struct_insert(s, key, value)),
        ); // Return new struct
    }

    // Unsupported type
    type_error!(ctx, args[0], "put", "array, struct, set, bytes, or string")
}
