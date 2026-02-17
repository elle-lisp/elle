//! Struct operations primitives (immutable hash maps)
use crate::value::{Condition, TableKey, Value};
use std::collections::BTreeMap;

/// Convert a Value to a TableKey
fn value_to_table_key(val: &Value) -> Result<TableKey, Condition> {
    if val.is_nil() {
        Ok(TableKey::Nil)
    } else if let Some(b) = val.as_bool() {
        Ok(TableKey::Bool(b))
    } else if let Some(i) = val.as_int() {
        Ok(TableKey::Int(i))
    } else if let Some(sym_id) = val.as_symbol() {
        Ok(TableKey::Symbol(crate::value::SymbolId(sym_id)))
    } else if let Some(s) = val.as_string() {
        Ok(TableKey::String(s.to_string()))
    } else {
        Err(Condition::type_error(format!(
            "expected table key, got {}",
            val.type_name()
        )))
    }
}

/// Create an immutable struct from key-value pairs
/// (struct key1 val1 key2 val2 ...)
pub fn prim_struct(args: &[Value]) -> Result<Value, Condition> {
    if !args.len().is_multiple_of(2) {
        return Err(Condition::error(
            "struct: requires an even number of arguments (key-value pairs)".to_string(),
        ));
    }

    let mut map = BTreeMap::new();
    for i in (0..args.len()).step_by(2) {
        let key = value_to_table_key(&args[i])?;
        let value = args[i + 1];
        map.insert(key, value);
    }

    Ok(Value::struct_from(map))
}

/// Get a value from a struct by key
/// `(struct-get struct key [default])`
pub fn prim_struct_get(args: &[Value]) -> Result<Value, Condition> {
    if args.len() < 2 || args.len() > 3 {
        return Err(Condition::arity_error(format!(
            "struct-get: expected 2-3 arguments, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-get: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;
    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    Ok(s.get(&key).copied().unwrap_or(default))
}

/// Create a new struct with an updated key-value pair (immutable)
/// (struct-put struct key value) returns a new struct
pub fn prim_struct_put(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 3 {
        return Err(Condition::arity_error(format!(
            "struct-put: expected 3 arguments, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-put: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;
    let value = args[2];

    let mut new_map = s.clone();
    new_map.insert(key, value);
    Ok(Value::struct_from(new_map))
}

/// Create a new struct without a key (immutable)
/// (struct-del struct key) returns a new struct
pub fn prim_struct_del(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "struct-del: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-del: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;

    let mut new_map = s.clone();
    new_map.remove(&key);
    Ok(Value::struct_from(new_map))
}

/// Get all keys from a struct as a list
/// (struct-keys struct)
pub fn prim_struct_keys(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "struct-keys: expected 1 argument, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-keys: expected struct, got {}",
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
}

/// Get all values from a struct as a list
/// (struct-values struct)
pub fn prim_struct_values(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "struct-values: expected 1 argument, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-values: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    let values: Vec<Value> = s.values().copied().collect();
    Ok(crate::value::list(values))
}

/// Check if a struct has a key
/// (struct-has? struct key)
pub fn prim_struct_has(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "struct-has?: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-has?: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    let key = value_to_table_key(&args[1])?;

    Ok(Value::bool(s.contains_key(&key)))
}

/// Get the number of entries in a struct
/// (struct-length struct)
pub fn prim_struct_length(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "struct-length: expected 1 argument, got {}",
            args.len()
        )));
    }

    let s = args[0].as_struct().ok_or_else(|| {
        Condition::type_error(format!(
            "struct-length: expected struct, got {}",
            args[0].type_name()
        ))
    })?;
    Ok(Value::int(s.len() as i64))
}
