//! Table operations primitives (mutable hash tables)
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

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
        let key = match value_to_table_key(&args[i]) {
            Ok(k) => k,
            Err(e) => return (SIG_ERROR, e),
        };
        let value = args[i + 1];
        map.insert(key, value);
    }

    (SIG_OK, Value::table_from(map))
}

/// Convert a Value to a TableKey
fn value_to_table_key(val: &Value) -> Result<TableKey, Value> {
    if val.is_nil() {
        Ok(TableKey::Nil)
    } else if let Some(b) = val.as_bool() {
        Ok(TableKey::Bool(b))
    } else if let Some(i) = val.as_int() {
        Ok(TableKey::Int(i))
    } else if let Some(id) = val.as_symbol() {
        Ok(TableKey::Symbol(crate::value::SymbolId(id)))
    } else if let Some(s) = val.as_string() {
        Ok(TableKey::String(s.to_string()))
    } else {
        Err(error_val(
            "type-error",
            format!(
                "expected table key (nil, bool, int, symbol, or string), got {}",
                val.type_name()
            ),
        ))
    }
}

/// Get a value from a table by key
/// `(get table key [default])`
pub fn prim_table_get(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-get: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-get: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
    };
    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    let borrowed = table.borrow();
    (SIG_OK, borrowed.get(&key).copied().unwrap_or(default))
}

/// Put a key-value pair into a table (mutable, in-place)
/// (put table key value)
pub fn prim_table_put(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-put: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-put: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
    };
    let value = args[2];

    table.borrow_mut().insert(key, value);
    (SIG_OK, args[0]) // Return the table itself
}

/// Delete a key from a table (mutable, in-place)
/// (del table key)
pub fn prim_table_del(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-del: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-del: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
    };

    table.borrow_mut().remove(&key);
    (SIG_OK, args[0]) // Return the table itself
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

    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
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

/// Get all keys from a table as a list
/// (keys table)
pub fn prim_table_keys(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-keys: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-keys: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let borrowed = table.borrow();

    let keys: Vec<Value> = borrowed
        .keys()
        .map(|k| match k {
            TableKey::Nil => Value::NIL,
            TableKey::Bool(b) => Value::bool(*b),
            TableKey::Int(i) => Value::int(*i),
            TableKey::Symbol(sid) => Value::symbol(sid.0),
            TableKey::String(s) => Value::string(s.as_str()),
        })
        .collect();

    (SIG_OK, crate::value::list(keys))
}

/// Get all values from a table as a list
/// (values table)
pub fn prim_table_values(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-values: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-values: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let borrowed = table.borrow();

    let values: Vec<Value> = borrowed.values().copied().collect();
    (SIG_OK, crate::value::list(values))
}

/// Check if a table has a key
/// (has-key? table key)
pub fn prim_table_has(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-has?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-has?: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
    };

    (SIG_OK, Value::bool(table.borrow().contains_key(&key)))
}

/// Get the number of entries in a table
/// (length table)
pub fn prim_table_length(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table-length: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let table = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("table-length: expected table, got {}", args[0].type_name()),
                ),
            )
        }
    };
    (SIG_OK, Value::int(table.borrow().len() as i64))
}

// ============ POLYMORPHIC FUNCTIONS (work on both tables and structs) ============

/// Polymorphic get - works on both tables and structs
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
        let key = match value_to_table_key(&args[1]) {
            Ok(k) => k,
            Err(e) => return (SIG_ERROR, e),
        };
        let borrowed = table.borrow();
        (SIG_OK, borrowed.get(&key).copied().unwrap_or(default))
    } else if args[0].is_struct() {
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
        let key = match value_to_table_key(&args[1]) {
            Ok(k) => k,
            Err(e) => return (SIG_ERROR, e),
        };
        (SIG_OK, s.get(&key).copied().unwrap_or(default))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("get: expected table or struct, got {}", args[0].type_name()),
            ),
        )
    }
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
        let keys: Vec<Value> = borrowed
            .keys()
            .map(|k| match k {
                TableKey::Nil => Value::NIL,
                TableKey::Bool(b) => Value::bool(*b),
                TableKey::Int(i) => Value::int(*i),
                TableKey::Symbol(sid) => Value::symbol(sid.0),
                TableKey::String(s) => Value::string(s.as_str()),
            })
            .collect();
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
        let keys: Vec<Value> = s
            .keys()
            .map(|k| match k {
                TableKey::Nil => Value::NIL,
                TableKey::Bool(b) => Value::bool(*b),
                TableKey::Int(i) => Value::int(*i),
                TableKey::Symbol(sid) => Value::symbol(sid.0),
                TableKey::String(st) => Value::string(st.as_str()),
            })
            .collect();
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

    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
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

/// Polymorphic put - works on both tables and structs
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

    let key = match value_to_table_key(&args[1]) {
        Ok(k) => k,
        Err(e) => return (SIG_ERROR, e),
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
        (SIG_OK, args[0]) // Return the mutated table
    } else if args[0].is_struct() {
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
        (SIG_OK, Value::struct_from(new_map)) // Return new struct
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("put: expected table or struct, got {}", args[0].type_name()),
            ),
        )
    }
}
