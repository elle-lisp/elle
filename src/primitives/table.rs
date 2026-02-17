//! Table operations primitives (mutable hash tables)
use crate::value::{Condition, TableKey, Value};
use std::collections::BTreeMap;

/// Create a mutable table from key-value pairs
/// (table key1 val1 key2 val2 ...)
pub fn prim_table(args: &[Value]) -> Result<Value, Condition> {
    if !args.len().is_multiple_of(2) {
        return Err(Condition::error(
            "table: requires an even number of arguments (key-value pairs)".to_string(),
        ));
    }

    let mut map = BTreeMap::new();
    for i in (0..args.len()).step_by(2) {
        let key = value_to_table_key(&args[i])?;
        let value = args[i + 1];
        map.insert(key, value);
    }

    Ok(Value::table_from(map))
}

/// Convert a Value to a TableKey
fn value_to_table_key(val: &Value) -> Result<TableKey, Condition> {
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
        Err(Condition::type_error(format!(
            "expected table key (nil, bool, int, symbol, or string), got {}",
            val.type_name()
        )))
    }
}

/// Get a value from a table by key
/// `(get table key [default])`
pub fn prim_table_get(args: &[Value]) -> Result<Value, Condition> {
    if args.len() < 2 || args.len() > 3 {
        return Err(Condition::arity_error(format!(
            "table-get: expected 2-3 arguments, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-get: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;
    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    let borrowed = table.borrow();
    Ok(borrowed.get(&key).copied().unwrap_or(default))
}

/// Put a key-value pair into a table (mutable, in-place)
/// (put table key value)
pub fn prim_table_put(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(format!(
            "table-put: expected 3 arguments, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-put: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;
    let value = args[2];

    table.borrow_mut().insert(key, value);
    Ok(args[0]) // Return the table itself
}

/// Delete a key from a table (mutable, in-place)
/// (del table key)
pub fn prim_table_del(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "table-del: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-del: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;

    table.borrow_mut().remove(&key);
    Ok(args[0]) // Return the table itself
}

/// Polymorphic del - works on both tables and structs
/// For tables: mutates in-place and returns the table
/// For structs: returns a new struct without the field (immutable)
/// `(del collection key)`
pub fn prim_del(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "del: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let key = value_to_table_key(&args[1])?;

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!("del: expected table, got {}", args[0].type_name()))
        })?;
        table.borrow_mut().remove(&key);
        Ok(args[0]) // Return the mutated table
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!("del: expected struct, got {}", args[0].type_name()))
        })?;
        let mut new_map = s.clone();
        new_map.remove(&key);
        Ok(Value::struct_from(new_map)) // Return new struct
    } else {
        Err(Condition::type_error(format!(
            "del: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}

/// Get all keys from a table as a list
/// (keys table)
pub fn prim_table_keys(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "table-keys: expected 1 argument, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-keys: expected table, got {}",
            args[0].type_name()
        ))
    })?;
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

    Ok(crate::value::list(keys))
}

/// Get all values from a table as a list
/// (values table)
pub fn prim_table_values(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "table-values: expected 1 argument, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-values: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    let borrowed = table.borrow();

    let values: Vec<Value> = borrowed.values().copied().collect();
    Ok(crate::value::list(values))
}

/// Check if a table has a key
/// (has-key? table key)
pub fn prim_table_has(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "table-has?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-has?: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;

    Ok(Value::bool(table.borrow().contains_key(&key)))
}

/// Get the number of entries in a table
/// (length table)
pub fn prim_table_length(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "table-length: expected 1 argument, got {}",
            args.len()
        )));
    }

    let table = args[0].as_table().ok_or_else(|| {
        Condition::type_error(format!(
            "table-length: expected table, got {}",
            args[0].type_name()
        ))
    })?;
    Ok(Value::int(table.borrow().len() as i64))
}

// ============ POLYMORPHIC FUNCTIONS (work on both tables and structs) ============

/// Polymorphic get - works on both tables and structs
/// `(get collection key [default])`
pub fn prim_get(args: &[Value]) -> Result<Value, Condition> {
    if args.len() < 2 || args.len() > 3 {
        return Err(Condition::arity_error(format!(
            "get: expected 2-3 arguments, got {}",
            args.len()
        )));
    }

    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!("get: expected table, got {}", args[0].type_name()))
        })?;
        let key = value_to_table_key(&args[1])?;
        let borrowed = table.borrow();
        Ok(borrowed.get(&key).copied().unwrap_or(default))
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!("get: expected struct, got {}", args[0].type_name()))
        })?;
        let key = value_to_table_key(&args[1])?;
        Ok(s.get(&key).copied().unwrap_or(default))
    } else {
        Err(Condition::type_error(format!(
            "get: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}

/// Polymorphic keys - works on both tables and structs
/// `(keys collection)`
pub fn prim_keys(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "keys: expected 1 argument, got {}",
            args.len()
        )));
    }

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!("keys: expected table, got {}", args[0].type_name()))
        })?;
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
        Ok(crate::value::list(keys))
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!(
                "keys: expected struct, got {}",
                args[0].type_name()
            ))
        })?;
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
        Ok(crate::value::list(keys))
    } else {
        Err(Condition::type_error(format!(
            "keys: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}

/// Polymorphic values - works on both tables and structs
/// `(values collection)`
pub fn prim_values(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "values: expected 1 argument, got {}",
            args.len()
        )));
    }

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!(
                "values: expected table, got {}",
                args[0].type_name()
            ))
        })?;
        let borrowed = table.borrow();
        let values: Vec<Value> = borrowed.values().copied().collect();
        Ok(crate::value::list(values))
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!(
                "values: expected struct, got {}",
                args[0].type_name()
            ))
        })?;
        let values: Vec<Value> = s.values().copied().collect();
        Ok(crate::value::list(values))
    } else {
        Err(Condition::type_error(format!(
            "values: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}

/// Polymorphic has-key? - works on both tables and structs
/// `(has-key? collection key)`
pub fn prim_has_key(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "has-key?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let key = value_to_table_key(&args[1])?;

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!(
                "has-key?: expected table, got {}",
                args[0].type_name()
            ))
        })?;
        Ok(Value::bool(table.borrow().contains_key(&key)))
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!(
                "has-key?: expected struct, got {}",
                args[0].type_name()
            ))
        })?;
        Ok(Value::bool(s.contains_key(&key)))
    } else {
        Err(Condition::type_error(format!(
            "has-key?: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}

/// Polymorphic put - works on both tables and structs
/// For tables: mutates in-place and returns the table
/// For structs: returns a new struct with the updated field (immutable)
/// `(put collection key value)`
pub fn prim_put(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(format!(
            "put: expected 3 arguments, got {}",
            args.len()
        )));
    }

    let key = value_to_table_key(&args[1])?;
    let value = args[2];

    if args[0].is_table() {
        let table = args[0].as_table().ok_or_else(|| {
            Condition::type_error(format!("put: expected table, got {}", args[0].type_name()))
        })?;
        table.borrow_mut().insert(key, value);
        Ok(args[0]) // Return the mutated table
    } else if args[0].is_struct() {
        let s = args[0].as_struct().ok_or_else(|| {
            Condition::type_error(format!("put: expected struct, got {}", args[0].type_name()))
        })?;
        let mut new_map = s.clone();
        new_map.insert(key, value);
        Ok(Value::struct_from(new_map)) // Return new struct
    } else {
        Err(Condition::type_error(format!(
            "put: expected table or struct, got {}",
            args[0].type_name()
        )))
    }
}
