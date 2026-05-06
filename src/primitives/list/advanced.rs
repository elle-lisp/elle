//! Advanced list operations: append, concat, take, drop, butlast, reverse, last
use crate::primitives::collection::coll_combine;
use crate::primitives::seq::{seq_butlast, seq_last, seq_reverse};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, list, Value};

/// Take the first n elements of a list
pub(crate) fn prim_take(args: &[Value]) -> (SignalBits, Value) {
    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "argument-error",
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

/// Get all elements of a sequence except the last
pub(crate) fn prim_butlast(args: &[Value]) -> (SignalBits, Value) {
    match seq_butlast(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Append two collections
pub(crate) fn prim_append(args: &[Value]) -> (SignalBits, Value) {
    // @string (mutable) — accepts string or @string as second arg
    // append has special mutation semantics for mutable types that
    // coll_combine doesn't handle (mutate first arg in place).
    if let Some(buf_ref) = args[0].as_string_mut() {
        if let Some(other_buf) = args[1].as_string_mut() {
            let other = other_buf.borrow();
            buf_ref.borrow_mut().extend(other.iter());
        } else if let Some(other) = args[1].with_string(|s| s.as_bytes().to_vec()) {
            buf_ref.borrow_mut().extend(other.iter());
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: expected string or @string, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
        return (SIG_OK, args[0]);
    }

    // @array (mutable) — accepts array or @array
    if let Some(vec_ref) = args[0].as_array_mut() {
        if let Some(other_vec_ref) = args[1].as_array_mut() {
            let other = other_vec_ref.borrow();
            vec_ref.borrow_mut().extend(other.iter().cloned());
        } else if let Some(other) = args[1].as_array() {
            vec_ref.borrow_mut().extend(other.iter().cloned());
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: expected array or @array, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
        return (SIG_OK, args[0]);
    }

    // @bytes (mutable) — accepts bytes or @bytes
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        if let Some(other) = args[1].as_bytes_mut() {
            let o = other.borrow();
            blob_ref.borrow_mut().extend(o.iter());
        } else if let Some(other) = args[1].as_bytes() {
            blob_ref.borrow_mut().extend(other.iter());
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: expected bytes or @bytes, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
        return (SIG_OK, args[0]);
    }

    // List (or syntax list — used during macro expansion)
    if args[0].is_pair() || args[0].is_empty_list() || args[0].as_syntax().is_some() {
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
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::pair(val, result);
        }
        return (SIG_OK, result);
    }

    // Remaining immutable types — delegate to coll_combine
    match coll_combine(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Concatenate one or more collections of the same type.
/// Collects all elements in a single pass, then builds the result once.
pub(crate) fn prim_concat(args: &[Value]) -> (SignalBits, Value) {
    // No arguments: return empty list (identity element for concat)
    if args.is_empty() {
        return (SIG_OK, Value::EMPTY_LIST);
    }

    // Single argument: return as-is (identity)
    if args.len() == 1 {
        return (SIG_OK, args[0]);
    }

    // Dispatch on the type of the first argument
    // @string
    if args[0].as_string_mut().is_some() {
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
    if args[0].is_pair() || args[0].is_empty_list() {
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
            result = Value::pair(val, result);
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
    match seq_reverse(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Get the last element of a sequence
pub(crate) fn prim_last(args: &[Value]) -> (SignalBits, Value) {
    match seq_last(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Drop the first n elements of a list
pub(crate) fn prim_drop(args: &[Value]) -> (SignalBits, Value) {
    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "argument-error",
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
