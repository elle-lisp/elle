//! Table operations primitives (mutable hash tables)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

/// Declarative table of table primitives.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "table",
        func: prim_table,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable table from key-value pairs",
        params: &[],
        category: "table",
        example: "(table :a 1 :b 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "get",
        func: prim_get,
        effect: Effect::none(),
        arity: Arity::Range(2, 3),
        doc: "Get a value from a collection (tuple, array, string, table, or struct) by index or key, with optional default",
        params: &["collection", "key", "default"],
        category: "table",
        example: "(get [1 2 3] 0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "put",
        func: prim_put,
        effect: Effect::none(),
        arity: Arity::Exact(3),
        doc: "Put a key-value pair into a table or struct",
        params: &["collection", "key", "value"],
        category: "table",
        example: "(put (table) :a 1)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "del",
        func: prim_del,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Delete a key from a table or struct",
        params: &["collection", "key"],
        category: "table",
        example: "(del (table :a 1) :a)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keys",
        func: prim_keys,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get all keys from a table or struct as a list",
        params: &["collection"],
        category: "table",
        example: "(keys (table :a 1 :b 2))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "values",
        func: prim_values,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get all values from a table or struct as a list",
        params: &["collection"],
        category: "table",
        example: "(values (table :a 1 :b 2))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "has-key?",
        func: prim_has_key,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Check if a table or struct has a key",
        params: &["collection", "key"],
        category: "table",
        example: "(has-key? (table :a 1) :a)",
        aliases: &[],
    },
];

/// Create a mutable table from key-value pairs
/// (table key1 val1 key2 val2 ...)
pub fn prim_table(args: &[Value]) -> (SignalBits, Value) {
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "table: requires an even number of arguments (key-value pairs)".to_string(),
            ),
        );
    }

    let mut map = BTreeMap::new();
    for i in (0..args.len()).step_by(2) {
        let key = match TableKey::from_value(&args[i]) {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("expected hashable value, got {}", args[i].type_name()),
                    ),
                )
            }
        };
        let value = args[i + 1];
        map.insert(key, value);
    }

    (SIG_OK, Value::table_from(map))
}

/// Polymorphic del - works on both tables and structs
/// For tables: mutates in-place and returns the table
/// For structs: returns a new struct without the field (immutable)
/// `(del collection key)`
pub fn prim_del(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("del: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

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

    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("del: expected table, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        table.borrow_mut().remove(&key);
        (SIG_OK, args[0]) // Return the mutated table
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("del: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let mut new_map = s.clone();
        new_map.remove(&key);
        (SIG_OK, Value::struct_from(new_map)) // Return new struct
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("del: expected table or struct, got {}", args[0].type_name()),
            ),
        )
    }
}

// ============ POLYMORPHIC FUNCTIONS (work on both tables and structs) ============

/// Polymorphic get - works on tuples, arrays, strings, tables, and structs
/// `(get collection key [default])`
pub fn prim_get(args: &[Value]) -> (SignalBits, Value) {
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
    if let Some(vec_ref) = args[0].as_array() {
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
    if let Some(elems) = args[0].as_tuple() {
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
    if let Some(buf_ref) = args[0].as_buffer() {
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
        match s.chars().nth(index as usize) {
            Some(ch) => {
                let ch_str = ch.to_string();
                return (SIG_OK, Value::string(ch_str.as_str()));
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
    if let Some(blob_ref) = args[0].as_blob() {
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
                match s.chars().nth(index as usize) {
                    Some(ch) => {
                        let ch_str = ch.to_string();
                        (SIG_OK, Value::string(ch_str.as_str()))
                    }
                    None => (SIG_OK, default),
                }
            })
            .unwrap();
    }

    // Table (mutable keyed collection)
    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("get: expected table, got {}", args[0].type_name()),
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
        let borrowed = table.borrow();
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
                "get: expected collection (list, tuple, array, string, buffer, table, or struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Polymorphic keys - works on both tables and structs
/// `(keys collection)`
pub fn prim_keys(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keys: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("keys: expected table, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let borrowed = table.borrow();
        let keys: Vec<Value> = borrowed.keys().map(|k| k.to_value()).collect();
        (SIG_OK, crate::value::list(keys))
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("keys: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let keys: Vec<Value> = s.keys().map(|k| k.to_value()).collect();
        (SIG_OK, crate::value::list(keys))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "keys: expected table or struct, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Polymorphic values - works on both tables and structs
/// `(values collection)`
pub fn prim_values(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("values: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("values: expected table, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let borrowed = table.borrow();
        let values: Vec<Value> = borrowed.values().copied().collect();
        (SIG_OK, crate::value::list(values))
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("values: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        let values: Vec<Value> = s.values().copied().collect();
        (SIG_OK, crate::value::list(values))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "values: expected table or struct, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Polymorphic has-key? - works on both tables and structs
/// `(has-key? collection key)`
pub fn prim_has_key(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("has-key?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

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

    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("has-key?: expected table, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        (SIG_OK, Value::bool(table.borrow().contains_key(&key)))
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("has-key?: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        (SIG_OK, Value::bool(s.contains_key(&key)))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "has-key?: expected table or struct, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Polymorphic put - works on tuples, arrays, strings, tables, and structs
/// For arrays: mutates in-place and returns the array
/// For tuples: returns a new tuple with the updated element (immutable)
/// For strings: returns a new string with the updated character (immutable)
/// For tables: mutates in-place and returns the table
/// For structs: returns a new struct with the updated field (immutable)
/// `(put collection key value)`
pub fn prim_put(args: &[Value]) -> (SignalBits, Value) {
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
    if let Some(buf_ref) = args[0].as_buffer() {
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
    if let Some(blob_ref) = args[0].as_blob() {
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
    if let Some(vec_ref) = args[0].as_array() {
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
    if let Some(elems) = args[0].as_tuple() {
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
        return (SIG_OK, Value::tuple(new_elems));
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
                let chars: Vec<char> = s.chars().collect();
                if index < 0 || index as usize >= chars.len() {
                    return (
                        SIG_ERROR,
                        error_val(
                            "error",
                            format!(
                                "put: index {} out of bounds (length {})",
                                index,
                                chars.len()
                            ),
                        ),
                    );
                }
                let mut result = String::new();
                for (i, ch) in chars.iter().enumerate() {
                    if i == index as usize {
                        result.push_str(&replacement);
                    } else {
                        result.push(*ch);
                    }
                }
                (SIG_OK, Value::string(result.as_str()))
            })
            .unwrap();
    }

    // Table (mutable keyed collection) - mutate in place
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

    if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("put: expected table, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        table.borrow_mut().insert(key, value);
        return (SIG_OK, args[0]); // Return the mutated table
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
                "put: expected collection (tuple, array, string, buffer, table, or struct), got {}",
                args[0].type_name()
            ),
        ),
    )
}
