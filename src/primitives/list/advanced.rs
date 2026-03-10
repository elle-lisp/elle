//! Advanced list operations: append, concat, reverse, last, butlast, take, drop
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, list, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Append multiple lists
/// Polymorphic append - works on arrays, tuples, and strings
/// For arrays: mutates first arg in place, returns it
/// For tuples: returns new tuple
/// For strings: returns new string
/// `(append collection1 collection2)`
pub(crate) fn prim_append(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("append: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Buffer (mutable) - mutate in place
    if let Some(buf_ref) = args[0].as_string_mut() {
        if let Some(other_buf_ref) = args[1].as_string_mut() {
            let other_borrowed = other_buf_ref.borrow();
            let mut borrowed = buf_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter());
            drop(borrowed);
            return (SIG_OK, args[0]); // Return the mutated buffer
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got buffer and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Array (mutable) - mutate in place
    if let Some(vec_ref) = args[0].as_array_mut() {
        if let Some(other_vec_ref) = args[1].as_array_mut() {
            let other_borrowed = other_vec_ref.borrow();
            let mut borrowed = vec_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter().cloned());
            drop(borrowed);
            return (SIG_OK, args[0]); // Return the mutated array
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got array and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Tuple (immutable) - return new tuple
    if let Some(elems) = args[0].as_array() {
        if let Some(other_elems) = args[1].as_array() {
            let mut result = elems.to_vec();
            result.extend(other_elems.iter().cloned());
            return (SIG_OK, Value::array(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got tuple and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // String (immutable) - return new string
    if args[0].is_string() {
        if args[1].is_string() {
            let s = args[0].with_string(|s| s.to_string()).unwrap();
            let other_s = args[1].with_string(|s| s.to_string()).unwrap();
            let mut result = s;
            result.push_str(&other_s);
            return (SIG_OK, Value::string(result.as_str()));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got string and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Bytes (immutable) - return new bytes
    if let Some(b) = args[0].as_bytes() {
        if let Some(other_b) = args[1].as_bytes() {
            let mut result = b.to_vec();
            result.extend(other_b);
            return (SIG_OK, Value::bytes(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got bytes and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Blob (mutable) - mutate in place
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        if let Some(other_blob_ref) = args[1].as_bytes_mut() {
            let other_borrowed = other_blob_ref.borrow();
            let mut borrowed = blob_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter());
            drop(borrowed);
            return (SIG_OK, args[0]);
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got blob and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // List (or syntax list — used during macro expansion)
    if args[0].is_cons() || args[0].is_empty_list() || args[0].as_syntax().is_some() {
        // list_to_vec handles both cons lists and syntax lists
        let mut first = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("append: {}", e))),
        };
        let second = match args[1].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "append: both arguments must be same type, got list and {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        first.extend(second);
        // Rebuild as a proper list
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::cons(val, result);
        }
        return (SIG_OK, result);
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "append: expected collection (list, array, tuple, or string), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Polymorphic concat - always returns new value, never mutates
/// Works on arrays, tuples, and strings
/// `(concat collection1 collection2)`
pub(crate) fn prim_concat(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("concat: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Buffer - return new buffer
    if let Some(buf_ref) = args[0].as_string_mut() {
        if let Some(other_buf_ref) = args[1].as_string_mut() {
            let borrowed = buf_ref.borrow();
            let other_borrowed = other_buf_ref.borrow();
            let mut result = borrowed.clone();
            result.extend(other_borrowed.iter());
            return (SIG_OK, Value::string_mut(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got buffer and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Array - return new array
    if let Some(vec_ref) = args[0].as_array_mut() {
        if let Some(other_vec_ref) = args[1].as_array_mut() {
            let borrowed = vec_ref.borrow();
            let other_borrowed = other_vec_ref.borrow();
            let mut result = borrowed.clone();
            result.extend(other_borrowed.iter().cloned());
            return (SIG_OK, Value::array_mut(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got array and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Tuple - return new tuple
    if let Some(elems) = args[0].as_array() {
        if let Some(other_elems) = args[1].as_array() {
            let mut result = elems.to_vec();
            result.extend(other_elems.iter().cloned());
            return (SIG_OK, Value::array(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got tuple and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // String - return new string
    if args[0].is_string() {
        if args[1].is_string() {
            let s = args[0].with_string(|s| s.to_string()).unwrap();
            let other_s = args[1].with_string(|s| s.to_string()).unwrap();
            let mut result = s;
            result.push_str(&other_s);
            return (SIG_OK, Value::string(result.as_str()));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got string and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // List (cons-based) - return new list
    if args[0].is_cons() || args[0].is_empty_list() {
        let mut first = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("concat: {}", e))),
        };
        let second = match args[1].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: both arguments must be same type, got list and {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        first.extend(second);
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::cons(val, result);
        }
        return (SIG_OK, result);
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "concat: expected collection (list, array, tuple, string, or buffer), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Reverse a sequence (list, tuple, array, string)
pub(crate) fn prim_reverse(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("reverse: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Array — return new array
    if let Some(arr) = args[0].as_array_mut() {
        let mut vec = arr.borrow().to_vec();
        vec.reverse();
        return (SIG_OK, Value::array_mut(vec));
    }
    // Tuple — return new tuple
    if let Some(elems) = args[0].as_array() {
        let mut vec = elems.to_vec();
        vec.reverse();
        return (SIG_OK, Value::array(vec));
    }
    // String — reverse grapheme clusters
    if let Some(result) = args[0].with_string(|s| {
        let reversed: String = s.graphemes(true).rev().collect();
        (SIG_OK, Value::string(reversed))
    }) {
        return result;
    }
    // List — existing behavior (fallback via list_to_vec)
    let mut vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(_) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "reverse: expected sequence (list, tuple, array, or string), got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    vec.reverse();
    (SIG_OK, list(vec))
}

/// Get the last element of a list
pub(crate) fn prim_last(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("last: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("last: {}", e))),
    };
    match vec.last().cloned() {
        Some(v) => (SIG_OK, v),
        None => (
            SIG_ERROR,
            error_val("error", "last: cannot get last of empty list".to_string()),
        ),
    }
}

/// Get all elements of a list except the last
pub(crate) fn prim_butlast(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("butlast: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("butlast: {}", e)),
            )
        }
    };
    if vec.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "butlast: cannot get butlast of empty list".to_string(),
            ),
        );
    }
    let init: Vec<Value> = vec[..vec.len() - 1].to_vec();
    (SIG_OK, list(init))
}

/// Take the first n elements of a list
pub(crate) fn prim_take(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("take: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("take: count must be non-negative, got {}", n),
                ),
            );
        }
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("take: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("take: {}", e))),
    };

    let taken: Vec<Value> = vec.into_iter().take(count).collect();
    (SIG_OK, list(taken))
}

/// Drop the first n elements of a list
pub(crate) fn prim_drop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("drop: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("drop: count must be non-negative, got {}", n),
                ),
            );
        }
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("drop: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("drop: {}", e))),
    };

    let dropped: Vec<Value> = vec.into_iter().skip(count).collect();
    (SIG_OK, list(dropped))
}
