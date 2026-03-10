//! Polymorphic collection access primitives (get, put).
//!
//! These functions work on multiple collection types:
//! - `get`: retrieves values from tuples, arrays, strings, buffers, bytes, blobs, lists, and structs
//! - `put`: updates values in arrays, tuples, strings, buffers, blobs, and structs

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, TableKey, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Polymorphic get - works on tuples, arrays, strings, and structs
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
        if index < 0 || index as usize >= borrowed.len() {
            return (SIG_OK, default);
        }
        return (SIG_OK, borrowed[index as usize]);
    }

    // Tuple (immutable indexed collection)
    if let Some(elems) = args[0].as_array() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "get: tuple index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        if index < 0 || index as usize >= elems.len() {
            return (SIG_OK, default);
        }
        return (SIG_OK, elems[index as usize]);
    }

    // Buffer (mutable string — indexed by character position)
    if let Some(buf_ref) = args[0].as_string_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "get: buffer index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        if index < 0 {
            return (SIG_OK, default);
        }
        let borrowed = buf_ref.borrow();
        let s = match std::str::from_utf8(&borrowed) {
            Ok(s) => s,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("get: buffer contains invalid UTF-8: {}", e),
                    ),
                )
            }
        };
        match s.graphemes(true).nth(index as usize) {
            Some(g) => {
                return (SIG_OK, Value::string(g));
            }
            None => return (SIG_OK, default),
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
        if index < 0 || index as usize >= b.len() {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("get: index {} out of bounds (length {})", index, b.len()),
                ),
            );
        }
        return (SIG_OK, Value::int(b[index as usize] as i64));
    }

    // Blob (mutable binary data — indexed by byte position)
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "get: blob index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let borrowed = blob_ref.borrow();
        if index < 0 || index as usize >= borrowed.len() {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!(
                        "get: index {} out of bounds (length {})",
                        index,
                        borrowed.len()
                    ),
                ),
            );
        }
        return (SIG_OK, Value::int(borrowed[index as usize] as i64));
    }

    // String (immutable character sequence)
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
                if index < 0 {
                    return (SIG_OK, default);
                }
                match s.graphemes(true).nth(index as usize) {
                    Some(g) => (SIG_OK, Value::string(g)),
                    None => (SIG_OK, default),
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
        return (SIG_OK, s.get(&key).copied().unwrap_or(default));
    }

    // List (cons-based)
    if args[0].is_cons() || args[0].is_empty_list() {
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
        if index < 0 {
            return (SIG_OK, default);
        }
        let mut current = args[0];
        let mut i = 0i64;
        loop {
            if current.is_empty_list() || current.is_nil() {
                return (SIG_OK, default);
            }
            if let Some(cons) = current.as_cons() {
                if i == index {
                    return (SIG_OK, cons.first);
                }
                current = cons.rest;
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
                "get: expected collection (list, tuple, array, string, buffer, or struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Polymorphic put - works on tuples, arrays, strings, and structs
/// For arrays: mutates in-place and returns the array
/// For tuples: returns a new tuple with the updated element (immutable)
/// For strings: returns a new string with the updated character (immutable)
/// For structs: mutates in-place (@struct) or returns new (struct)
/// `(put collection key value)`
pub(crate) fn prim_put(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("put: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    // Buffer (mutable byte sequence) - mutate in place
    if let Some(buf_ref) = args[0].as_string_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "put: buffer index must be integer, got {}",
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
                        "error",
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
                            "put: buffer value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let len = buf_ref.borrow().len();
        if index < 0 || (index as usize) >= len {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("put: index {} out of bounds (length {})", index, len),
                ),
            );
        }
        buf_ref.borrow_mut()[index as usize] = byte;
        return (SIG_OK, args[0]); // Return the mutated buffer
    }

    // Blob (mutable byte sequence) - mutate in place
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "put: blob index must be integer, got {}",
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
                        "error",
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
                            "put: blob value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let len = blob_ref.borrow().len();
        if index < 0 || (index as usize) >= len {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("put: index {} out of bounds (length {})", index, len),
                ),
            );
        }
        blob_ref.borrow_mut()[index as usize] = byte;
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
        if index < 0 || (index as usize) >= len {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("put: index {} out of bounds (length {})", index, len),
                ),
            );
        }
        vec_ref.borrow_mut()[index as usize] = args[2];
        return (SIG_OK, args[0]); // Return the mutated array
    }

    // Tuple (immutable indexed collection) - return new tuple
    if let Some(elems) = args[0].as_array() {
        let index = match args[1].as_int() {
            Some(i) => i,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "put: tuple index must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        if index < 0 || (index as usize) >= elems.len() {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!(
                        "put: index {} out of bounds (length {})",
                        index,
                        elems.len()
                    ),
                ),
            );
        }
        let mut new_elems = elems.to_vec();
        new_elems[index as usize] = args[2];
        return (SIG_OK, Value::array(new_elems));
    }

    // String (immutable character sequence) - return new string
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
                if index < 0 || index as usize >= graphemes.len() {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            format!(
                                "put: index {} out of bounds (length {})",
                                index,
                                graphemes.len()
                            ),
                        ),
                    );
                }
                let mut result = String::new();
                for (i, g) in graphemes.iter().enumerate() {
                    if i == index as usize {
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
        let mut new_map = s.clone();
        new_map.insert(key, value);
        return (SIG_OK, Value::struct_from(new_map)); // Return new struct
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "put: expected collection (tuple, array, string, buffer, or struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}
