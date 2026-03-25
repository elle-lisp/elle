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

    // @string (mutable) — accepts string or @string as second arg
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

    // array (immutable) — accepts array or @array
    if let Some(elems) = args[0].as_array() {
        let other = if let Some(other) = args[1].as_array() {
            other.to_vec()
        } else if let Some(other_ref) = args[1].as_array_mut() {
            other_ref.borrow().clone()
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
        };
        let mut result = elems.to_vec();
        result.extend(other);
        return (SIG_OK, Value::array(result));
    }

    // string (immutable) — accepts string or @string
    if args[0].is_string() {
        let s = args[0].with_string(|s| s.as_bytes().to_vec()).unwrap();
        let other = if let Some(o) = args[1].with_string(|s| s.as_bytes().to_vec()) {
            o
        } else if let Some(o) = args[1].as_string_mut() {
            o.borrow().clone()
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
        };
        let mut result = s;
        result.extend(other);
        return (
            SIG_OK,
            Value::string(std::str::from_utf8(&result).unwrap_or("")),
        );
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

    // bytes (immutable) — accepts bytes or @bytes
    if let Some(b) = args[0].as_bytes() {
        let other = if let Some(other) = args[1].as_bytes() {
            other.to_vec()
        } else if let Some(other_ref) = args[1].as_bytes_mut() {
            other_ref.borrow().clone()
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
        };
        let mut result = b.to_vec();
        result.extend(other);
        return (SIG_OK, Value::bytes(result));
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

/// Get the last element of a sequence (list, array, @array, string)
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
    // Cons cell — walk to the last element
    if args[0].as_cons().is_some() || args[0].is_empty_list() {
        let vec = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("last: {}", e))),
        };
        return match vec.last().cloned() {
            Some(v) => (SIG_OK, v),
            None => (SIG_OK, Value::NIL),
        };
    }
    // Array
    if let Some(elems) = args[0].as_array() {
        return if elems.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, elems[elems.len() - 1])
        };
    }
    // @Array
    if let Some(arr) = args[0].as_array_mut() {
        let borrowed = arr.borrow();
        return if borrowed.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, borrowed[borrowed.len() - 1])
        };
    }
    // String / @String — last grapheme cluster
    if let Some(result) = args[0].with_string(|s| match s.graphemes(true).next_back() {
        Some(g) => (SIG_OK, Value::string(g)),
        None => (SIG_OK, Value::NIL),
    }) {
        return result;
    }
    // Bytes
    if let Some(b) = args[0].as_bytes() {
        return if b.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(b[b.len() - 1] as i64))
        };
    }
    // @Bytes
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return if borrowed.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(borrowed[borrowed.len() - 1] as i64))
        };
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("last: expected sequence, got {}", args[0].type_name()),
        ),
    )
}

/// Get the second element of a sequence (list, array, @array, string)
pub(crate) fn prim_second(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("second: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Cons cell
    if let Some(cons) = args[0].as_cons() {
        if let Some(inner) = cons.rest.as_cons() {
            return (SIG_OK, inner.first);
        }
        return (SIG_OK, Value::NIL);
    }
    // Empty list
    if args[0].is_empty_list() {
        return (SIG_OK, Value::NIL);
    }
    // Array
    if let Some(elems) = args[0].as_array() {
        return if elems.len() < 2 {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, elems[1])
        };
    }
    // @Array
    if let Some(arr) = args[0].as_array_mut() {
        let borrowed = arr.borrow();
        return if borrowed.len() < 2 {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, borrowed[1])
        };
    }
    // String / @String — second grapheme cluster
    if let Some(result) = args[0].with_string(|s| {
        let mut iter = s.graphemes(true);
        iter.next(); // skip first
        match iter.next() {
            Some(g) => (SIG_OK, Value::string(g)),
            None => (SIG_OK, Value::NIL),
        }
    }) {
        return result;
    }
    // Bytes
    if let Some(b) = args[0].as_bytes() {
        return if b.len() < 2 {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(b[1] as i64))
        };
    }
    // @Bytes
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return if borrowed.len() < 2 {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(borrowed[1] as i64))
        };
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("second: expected sequence, got {}", args[0].type_name()),
        ),
    )
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
                "argument-error",
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
