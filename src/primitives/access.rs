//! Polymorphic collection access primitives (get, put).
//!
//! These functions work on multiple collection types:
//! - `get`: retrieves values from arrays, @arrays, strings, @strings, bytes, @bytes, lists, and structs
//! - `put`: updates values in @arrays, arrays, strings, @strings, @bytes, and structs

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::fiberheap;
use crate::value::{error_val, sorted_struct_get, sorted_struct_insert, TableKey, Value};
use unicode_segmentation::UnicodeSegmentation;

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

/// Polymorphic get - works on arrays, @arrays, strings, @strings, and structs
/// `(get collection key [default])`
pub(crate) fn prim_get(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("get: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    // Array (mutable indexed collection)
    if let Some(vec_ref) = args[0].as_array_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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
                    error_val(
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
                    error_val(
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
                    error_val(
                        "encoding-error",
                        format!("get: @string contains invalid UTF-8: {}", e),
                    ),
                )
            }
        };
        if index >= 0 {
            match s.graphemes(true).nth(index as usize) {
                Some(g) => return (SIG_OK, Value::string(g)),
                None => return (SIG_OK, default),
            }
        } else {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            match resolve_index(index, graphemes.len()) {
                Some(i) => return (SIG_OK, Value::string(graphemes[i])),
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
                    error_val(
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
                    error_val(
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
                    error_val(
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
                    error_val(
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
                            error_val(
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
                        Some(g) => (SIG_OK, Value::string(g)),
                        None => (SIG_OK, default),
                    }
                } else {
                    let graphemes: Vec<&str> = s.graphemes(true).collect();
                    match resolve_index(index, graphemes.len()) {
                        Some(i) => (SIG_OK, Value::string(graphemes[i])),
                        None => (SIG_OK, default),
                    }
                }
            })
            .unwrap();
    }

    // Struct (mutable keyed collection)
    if args[0].is_struct_mut() {
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("get: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let key = match TableKey::from_value(&args[1]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("expected hashable value, got {}", args[1].type_name()),
                    ),
                )
            }
        };
        let borrowed = mstruct.borrow();
        return (SIG_OK, borrowed.get(&key).copied().unwrap_or(default));
    }

    // Struct (immutable keyed collection)
    if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("get: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let key = match TableKey::from_value(&args[1]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("expected hashable value, got {}", args[1].type_name()),
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
                    error_val(
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
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "get: expected collection (list, array, @array, string, @string, or struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Polymorphic put - works on arrays, @arrays, strings, @strings, and structs
/// For @arrays: mutates in-place and returns the @array
/// For arrays: returns a new array with the updated element (immutable)
/// For strings: returns a new string with the updated grapheme cluster (immutable)
/// For structs: mutates in-place (@struct) or returns new (struct)
/// `(put collection key value)`
pub(crate) fn prim_put(args: &[Value]) -> (SignalBits, Value) {
    // 2-arg form: (put set value) — delegates to add
    if args.len() == 2 {
        if args[0].is_set() || args[0].is_set_mut() {
            return crate::primitives::sets::prim_add(args);
        }
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "put: 2-argument form requires a set, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("put: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    // @string (mutable string — indexed by grapheme cluster position)
    if let Some(buf_ref) = args[0].as_string_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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
                    error_val(
                        "type-error",
                        format!(
                            "put: @string value must be string, got {}",
                            args[2].type_name()
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
                    error_val(
                        "encoding-error",
                        format!("put: @string contains invalid UTF-8: {}", e),
                    ),
                )
            }
        };
        let graphemes: Vec<&str> = s.graphemes(true).collect();
        let resolved = match resolve_index(index, graphemes.len()) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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
        drop(borrowed); // release immutable borrow
        *buf_ref.borrow_mut() = result.into_bytes(); // take mutable borrow
        return (SIG_OK, args[0]);
    }

    // @bytes (mutable byte sequence) - mutate in place
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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
                    error_val(
                        "argument-error",
                        format!("put: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "put: @bytes value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let len = blob_ref.borrow().len();
        let resolved = match resolve_index(index, len) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("put: index {} out of bounds (length {})", index, len),
                    ),
                );
            }
        };
        blob_ref.borrow_mut()[resolved] = byte;
        return (SIG_OK, args[0]);
    }

    // Array (mutable indexed collection) - mutate in place
    if let Some(vec_ref) = args[0].as_array_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "put: array index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let len = vec_ref.borrow().len();
        let resolved = match resolve_index(index, len) {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("put: index {} out of bounds (length {})", index, len),
                    ),
                );
            }
        };
        let old = vec_ref.borrow()[resolved];
        fiberheap::decref_and_free(old);
        fiberheap::incref(args[2]);
        vec_ref.borrow_mut()[resolved] = args[2];
        return (SIG_OK, args[0]); // Return the mutated array
    }

    // Array (immutable indexed collection) - return new array
    if let Some(elems) = args[0].as_array() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
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
                    error_val(
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
        return (SIG_OK, Value::array(new_elems));
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
                            error_val(
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
                            error_val(
                                "type-error",
                                format!(
                                    "put: string value must be string, got {}",
                                    args[2].type_name()
                                ),
                            ),
                        )
                    }
                };
                let graphemes: Vec<&str> = s.graphemes(true).collect();
                let resolved = match resolve_index(index, graphemes.len()) {
                    Some(i) => i,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
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
                (SIG_OK, Value::string(result.as_str()))
            })
            .unwrap();
    }

    // Struct (mutable keyed collection) - mutate in place
    let key = match TableKey::from_value(&args[1]) {
        Some(k) => k,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("expected hashable value, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let value = args[2];

    if args[0].is_struct_mut() {
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("put: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        // Decref old value, incref new value.
        if let Some(&old_val) = mstruct.borrow().get(&key) {
            fiberheap::decref_and_free(old_val);
        }
        fiberheap::incref(value);
        mstruct.borrow_mut().insert(key, value);
        return (SIG_OK, args[0]); // Return the mutated struct
    }

    // Struct (immutable keyed collection) - return new struct
    if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("put: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        return (
            SIG_OK,
            Value::struct_from_sorted(sorted_struct_insert(s, key, value)),
        ); // Return new struct
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "put: expected array, struct, set, bytes, or string, got {}",
                args[0].type_name()
            ),
        ),
    )
}
