//! Array operations primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::fiberheap;
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create an array from arguments
pub(crate) fn prim_array(args: &[Value]) -> (SignalBits, Value) {
    for &val in args {
        fiberheap::incref(val);
    }
    (SIG_OK, Value::array_mut(args.to_vec()))
}

/// Create an immutable array from arguments
pub(crate) fn prim_tuple(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::array(args.to_vec()))
}

/// Create a mutable array of n elements, all set to fill.
///
/// Complements `@array` (which takes explicit elements) by supporting
/// pre-allocation of a fixed-size array with a uniform initial value.
/// Returns @array (mutable), not array (immutable).
pub(crate) fn prim_array_new(args: &[Value]) -> (SignalBits, Value) {
    let n = match args[0].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        "array/new: size must be non-negative".to_string(),
                    ),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array/new: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let fill = args[1];
    // Each slot in the array holds a reference to fill.
    for _ in 0..n {
        fiberheap::incref(fill);
    }
    let vec = vec![fill; n];
    (SIG_OK, Value::array_mut(vec))
}

/// Push a value onto the end of an array or @string (mutates in place, returns the collection)
pub(crate) fn prim_push(args: &[Value]) -> (SignalBits, Value) {
    if let Some(vec_ref) = args[0].as_array_mut() {
        fiberheap::incref(args[1]);
        let mut vec = vec_ref.borrow_mut();
        vec.push(args[1]);
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let s = match args[1].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "push: @string value must be string, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        buf_ref.borrow_mut().extend_from_slice(s.as_bytes());
        return (SIG_OK, args[0]);
    }

    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let byte = match args[1].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("push: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "push: @bytes value must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        blob_ref.borrow_mut().push(byte);
        return (SIG_OK, args[0]);
    }

    // Immutable array — return new array with element appended
    if let Some(elems) = args[0].as_array() {
        let mut new = elems.to_vec();
        new.push(args[1]);
        return (SIG_OK, Value::array(new));
    }

    // Immutable string — return new string with value appended
    if args[0].is_string() {
        let s = match args[1].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "push: string value must be string, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        return args[0]
            .with_string(|base| {
                let mut new = base.to_string();
                new.push_str(&s);
                (SIG_OK, Value::string(new))
            })
            .unwrap_or_else(|| {
                (
                    SIG_ERROR,
                    error_val("type-error", "push: unreachable string case".to_string()),
                )
            });
    }

    // Immutable bytes — return new bytes with byte appended
    if let Some(b) = args[0].as_bytes() {
        let byte = match args[1].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("push: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "push: bytes value must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        let mut new = b.to_vec();
        new.push(byte);
        return (SIG_OK, Value::bytes(new));
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "push: expected array, @array, string, @string, bytes, or @bytes, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Pop a value from the end of an @array or @string (mutates in place, returns the removed element)
pub(crate) fn prim_pop(args: &[Value]) -> (SignalBits, Value) {
    if let Some(vec_ref) = args[0].as_array_mut() {
        let mut vec = vec_ref.borrow_mut();
        match vec.pop() {
            Some(v) => {
                drop(vec);
                // Decref: value leaves a durable collection reference.
                fiberheap::decref(v);
                return (SIG_OK, v);
            }
            None => {
                drop(vec);
                return (
                    SIG_ERROR,
                    error_val("argument-error", "pop: empty array".to_string()),
                );
            }
        }
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        if buf.is_empty() {
            drop(buf);
            return (
                SIG_ERROR,
                error_val("argument-error", "pop: empty @string".to_string()),
            );
        }
        let s = match std::str::from_utf8(&buf) {
            Ok(s) => s,
            Err(_) => {
                drop(buf);
                return (
                    SIG_ERROR,
                    error_val(
                        "encoding-error",
                        "pop: @string contains invalid UTF-8".to_string(),
                    ),
                );
            }
        };
        use unicode_segmentation::UnicodeSegmentation;
        let cluster = s.graphemes(true).next_back().unwrap().to_string();
        let new_len = buf.len() - cluster.len();
        buf.truncate(new_len);
        drop(buf);
        return (SIG_OK, Value::string(cluster));
    }

    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let mut blob = blob_ref.borrow_mut();
        match blob.pop() {
            Some(byte) => {
                drop(blob);
                return (SIG_OK, Value::int(byte as i64));
            }
            None => {
                drop(blob);
                return (
                    SIG_ERROR,
                    error_val("argument-error", "pop: empty @bytes".to_string()),
                );
            }
        }
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "pop: expected @array, @string, or @bytes, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Pop n values from the end of an @array or @string and return them as a new collection
pub(crate) fn prim_popn(args: &[Value]) -> (SignalBits, Value) {
    let n = match args[1].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        "popn: count must be non-negative".to_string(),
                    ),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("popn: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if let Some(vec_ref) = args[0].as_array_mut() {
        let mut vec = vec_ref.borrow_mut();
        let len = vec.len();
        let remove_count = std::cmp::min(n, len);
        // Decref values leaving the source @array.
        for i in (len - remove_count)..len {
            fiberheap::decref(vec[i]);
        }
        let removed: Vec<Value> = vec.drain(len - remove_count..).collect();
        drop(vec);
        // Incref values entering the new @array.
        for &val in &removed {
            fiberheap::incref(val);
        }
        return (SIG_OK, Value::array_mut(removed));
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        let len = buf.len();
        let remove_count = std::cmp::min(n, len);
        let removed: Vec<u8> = buf.drain(len - remove_count..).collect();
        drop(buf);
        return (SIG_OK, Value::string_mut(removed));
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "popn: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Insert a value at an index in an @array or @string (mutates in place, returns the collection)
pub(crate) fn prim_insert(args: &[Value]) -> (SignalBits, Value) {
    use crate::primitives::access::resolve_index;

    let raw_index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("insert: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if let Some(vec_ref) = args[0].as_array_mut() {
        let mut vec = vec_ref.borrow_mut();
        // insert allows index == len (append), so try resolve_index first,
        // then also accept len exactly for non-negative raw_index
        let index = match resolve_index(raw_index, vec.len()) {
            Some(i) => i,
            None => {
                // Allow index == len for append (only when non-negative or resolved == len)
                if raw_index >= 0 && raw_index as usize <= vec.len() {
                    raw_index as usize
                } else if raw_index < 0 {
                    return (
                        SIG_ERROR,
                        error_val(
                            "argument-error",
                            format!(
                                "insert: index {} out of bounds (length {})",
                                raw_index,
                                vec.len()
                            ),
                        ),
                    );
                } else {
                    vec.len() // clamp to append
                }
            }
        };
        // Incref: value enters a durable collection reference.
        fiberheap::incref(args[2]);
        vec.insert(index, args[2]);
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let byte = match args[2].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "argument-error",
                        format!("insert: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "insert: @string value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let mut buf = buf_ref.borrow_mut();
        let index = match resolve_index(raw_index, buf.len()) {
            Some(i) => i,
            None => {
                if raw_index >= 0 && raw_index as usize <= buf.len() {
                    raw_index as usize
                } else if raw_index < 0 {
                    return (
                        SIG_ERROR,
                        error_val(
                            "argument-error",
                            format!(
                                "insert: index {} out of bounds (length {})",
                                raw_index,
                                buf.len()
                            ),
                        ),
                    );
                } else {
                    buf.len()
                }
            }
        };
        buf.insert(index, byte);
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "insert: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Remove element(s) at an index from an @array or @string (mutates in place, returns the collection)
pub(crate) fn prim_remove(args: &[Value]) -> (SignalBits, Value) {
    use crate::primitives::access::resolve_index;

    let raw_index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("remove: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let count = if args.len() == 3 {
        match args[2].as_int() {
            Some(i) => {
                if i < 0 {
                    return (
                        SIG_ERROR,
                        error_val(
                            "argument-error",
                            "remove: count must be non-negative".to_string(),
                        ),
                    );
                }
                i as usize
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("remove: expected integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    } else {
        1
    };

    if let Some(vec_ref) = args[0].as_array_mut() {
        let mut vec = vec_ref.borrow_mut();
        if let Some(index) = resolve_index(raw_index, vec.len()) {
            let remove_count = std::cmp::min(count, vec.len() - index);
            for j in 0..remove_count {
                // Decref: value leaves a durable collection reference.
                fiberheap::decref(vec[index + j]);
            }
            for _ in 0..remove_count {
                vec.remove(index);
            }
        }
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        if let Some(index) = resolve_index(raw_index, buf.len()) {
            let remove_count = std::cmp::min(count, buf.len() - index);
            for _ in 0..remove_count {
                buf.remove(index);
            }
        }
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "remove: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "array",
        func: prim_tuple,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable array from arguments.",
        params: &[],
        category: "array",
        example: "(array 1 2 3) #=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "@array",
        func: prim_array,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable array from arguments.",
        params: &[],
        category: "array",
        example: "(@array 1 2 3) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "array/new",
        func: prim_array_new,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create array of n elements, all set to fill value.",
        params: &["n", "fill"],
        category: "array",
        example: "(array/new 3 0) #=> [0 0 0]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "push",
        func: prim_push,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Append element to end of array. Mutable: mutates in place. Immutable: returns new collection.",
        params: &["arr", "val"],
        category: "array",
        example: "(push @[1 2] 3) #=> @[1 2 3]\n(push [1 2] 3)  #=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pop",
        func: prim_pop,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove and return last element from array. Mutates in place.",
        params: &["arr"],
        category: "array",
        example: "(pop @[1 2 3]) #=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "popn",
        func: prim_popn,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove and return last n elements from array as a new array. Mutates original.",
        params: &["arr", "n"],
        category: "array",
        example: "(popn @[1 2 3 4] 2) #=> @[3 4]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "insert",
        func: prim_insert,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Insert element at index in array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "val"],
        category: "array",
        example: "(insert @[1 3] 1 2) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "remove",
        func: prim_remove,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Remove element(s) at index from array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "count"],
        category: "array",
        example: "(remove @[1 2 3] 1) #=> @[1 3]",
        aliases: &[],
    },
];
