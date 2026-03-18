//! Advanced list operations: append, concat, reverse, last, butlast, take, drop
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, list, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Append multiple lists
/// Polymorphic append - works on arrays and strings
/// For arrays: mutates first arg in place, returns it
/// For arrays: returns new array
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

    // @string (mutable) - mutate in place
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

    // Array (immutable) - return new array
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
                        "append: both arguments must be same type, got array and {}",
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

    // @bytes (mutable) - mutate in place
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
                "append: expected collection (list, array, or string), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Concatenate one or more collections of the same type.
/// Collects all elements in a single pass, then builds the result once.
pub(crate) fn prim_concat(args: &[Value]) -> (SignalBits, Value) {
    // Single argument: return as-is (identity)
    if args.len() == 1 {
        return (SIG_OK, args[0]);
    }

    // Dispatch on the type of the first argument
    // @string
    if args[0].as_string_mut().is_some() {
        // Pre-calculate total length
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_string_mut() {
                Some(buf_ref) => total_len += buf_ref.borrow().len(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected @string",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Vec::with_capacity(total_len);
        for arg in args {
            let buf_ref = arg.as_string_mut().unwrap();
            result.extend(buf_ref.borrow().iter());
        }
        return (SIG_OK, Value::string_mut(result));
    }

    // @array
    if args[0].as_array_mut().is_some() {
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_array_mut() {
                Some(vec_ref) => total_len += vec_ref.borrow().len(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected @array",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Vec::with_capacity(total_len);
        for arg in args {
            let vec_ref = arg.as_array_mut().unwrap();
            result.extend(vec_ref.borrow().iter().cloned());
        }
        return (SIG_OK, Value::array_mut(result));
    }

    // array (immutable)
    if args[0].as_array().is_some() {
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_array() {
                Some(elems) => total_len += elems.len(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected array",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Vec::with_capacity(total_len);
        for arg in args {
            let elems = arg.as_array().unwrap();
            result.extend(elems.iter().cloned());
        }
        return (SIG_OK, Value::array(result));
    }

    // string (immutable)
    if args[0].is_string() {
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            if arg.is_string() {
                arg.with_string(|s| total_len += s.len());
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: argument {} is {}, expected string",
                            i + 1,
                            arg.type_name()
                        ),
                    ),
                );
            }
        }
        let mut result = String::with_capacity(total_len);
        for arg in args {
            arg.with_string(|s| result.push_str(s));
        }
        return (SIG_OK, Value::string(result.as_str()));
    }

    // list (cons-based)
    if args[0].is_cons() || args[0].is_empty_list() {
        let mut all_elements = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            match arg.list_to_vec() {
                Ok(v) => all_elements.extend(v),
                Err(_) => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected list",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Value::EMPTY_LIST;
        for val in all_elements.into_iter().rev() {
            result = Value::cons(val, result);
        }
        return (SIG_OK, result);
    }

    // @bytes (mutable)
    if args[0].as_bytes_mut().is_some() {
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_bytes_mut() {
                Some(blob_ref) => total_len += blob_ref.borrow().len(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected @bytes",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Vec::with_capacity(total_len);
        for arg in args {
            let blob_ref = arg.as_bytes_mut().unwrap();
            result.extend(blob_ref.borrow().iter().copied());
        }
        return (SIG_OK, Value::bytes_mut(result));
    }

    // bytes (immutable)
    if args[0].as_bytes().is_some() {
        let mut total_len = 0usize;
        for (i, arg) in args.iter().enumerate() {
            match arg.as_bytes() {
                Some(slice) => total_len += slice.len(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "concat: argument {} is {}, expected bytes",
                                i + 1,
                                arg.type_name()
                            ),
                        ),
                    )
                }
            }
        }
        let mut result = Vec::with_capacity(total_len);
        for arg in args {
            let slice = arg.as_bytes().unwrap();
            result.extend(slice.iter().copied());
        }
        return (SIG_OK, Value::bytes(result));
    }

    // @set (mutable)
    if args[0].as_set_mut().is_some() {
        for (i, arg) in args.iter().enumerate() {
            if arg.as_set_mut().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: argument {} is {}, expected @set",
                            i + 1,
                            arg.type_name()
                        ),
                    ),
                );
            }
        }
        let mut result = std::collections::BTreeSet::new();
        for arg in args {
            let set_ref = arg.as_set_mut().unwrap();
            result.extend(set_ref.borrow().iter().copied());
        }
        return (SIG_OK, Value::set_mut(result));
    }

    // set (immutable)
    if args[0].as_set().is_some() {
        for (i, arg) in args.iter().enumerate() {
            if arg.as_set().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: argument {} is {}, expected set",
                            i + 1,
                            arg.type_name()
                        ),
                    ),
                );
            }
        }
        let mut result = std::collections::BTreeSet::new();
        for arg in args {
            let set = arg.as_set().unwrap();
            result.extend(set.iter().copied());
        }
        return (SIG_OK, Value::set(result));
    }

    // @struct (mutable)
    if args[0].as_struct_mut().is_some() {
        for (i, arg) in args.iter().enumerate() {
            if arg.as_struct_mut().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: argument {} is {}, expected @struct",
                            i + 1,
                            arg.type_name()
                        ),
                    ),
                );
            }
        }
        let mut result = std::collections::BTreeMap::new();
        for arg in args {
            let table_ref = arg.as_struct_mut().unwrap();
            result.extend(table_ref.borrow().iter().map(|(k, v)| (k.clone(), *v)));
        }
        return (SIG_OK, Value::struct_mut_from(result));
    }

    // struct (immutable)
    if args[0].as_struct().is_some() {
        for (i, arg) in args.iter().enumerate() {
            if arg.as_struct().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: argument {} is {}, expected struct",
                            i + 1,
                            arg.type_name()
                        ),
                    ),
                );
            }
        }
        let mut result = std::collections::BTreeMap::new();
        for arg in args {
            let map = arg.as_struct().unwrap();
            result.extend(map.iter().map(|(k, v)| (k.clone(), *v)));
        }
        return (SIG_OK, Value::struct_from(result));
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "concat: expected collection (list, array, @array, string, @string, bytes, @bytes, set, @set, struct, or @struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Reverse a sequence (list, array, @array, string)
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
    // Array — return new array
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
                        "reverse: expected sequence (list, array, or string), got {}",
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
