//! Struct operations primitives (immutable hash maps)
use crate::value::{TableKey, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

/// Create an immutable struct from key-value pairs
/// (struct key1 val1 key2 val2 ...)
pub fn prim_struct(args: &[Value]) -> Result<Value, String> {
    if !args.len().is_multiple_of(2) {
        return Err("struct requires an even number of arguments (key-value pairs)".to_string());
    }

    let mut map = BTreeMap::new();
    for i in (0..args.len()).step_by(2) {
        let key = TableKey::from_value(&args[i])?;
        let value = args[i + 1].clone();
        map.insert(key, value);
    }

    Ok(Value::Struct(Rc::new(map)))
}

/// Get a value from a struct by key
/// `(struct-get struct key [default])`
pub fn prim_struct_get(args: &[Value]) -> Result<Value, String> {
    if args.len() < 2 || args.len() > 3 {
        return Err("struct-get requires 2 or 3 arguments (struct, key, [default])".to_string());
    }

    let s = args[0].as_struct()?;
    let key = TableKey::from_value(&args[1])?;
    let default = if args.len() == 3 {
        args[2].clone()
    } else {
        Value::Nil
    };

    Ok(s.get(&key).cloned().unwrap_or(default))
}

/// Create a new struct with an updated key-value pair (immutable)
/// (struct-put struct key value) returns a new struct
pub fn prim_struct_put(args: &[Value]) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("struct-put requires exactly 3 arguments (struct, key, value)".to_string());
    }

    let s = args[0].as_struct()?;
    let key = TableKey::from_value(&args[1])?;
    let value = args[2].clone();

    let mut new_map = (**s).clone();
    new_map.insert(key, value);
    Ok(Value::Struct(Rc::new(new_map)))
}

/// Create a new struct without a key (immutable)
/// (struct-del struct key) returns a new struct
pub fn prim_struct_del(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("struct-del requires exactly 2 arguments (struct, key)".to_string());
    }

    let s = args[0].as_struct()?;
    let key = TableKey::from_value(&args[1])?;

    let mut new_map = (**s).clone();
    new_map.remove(&key);
    Ok(Value::Struct(Rc::new(new_map)))
}

/// Get all keys from a struct as a list
/// (struct-keys struct)
pub fn prim_struct_keys(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("struct-keys requires exactly 1 argument (struct)".to_string());
    }

    let s = args[0].as_struct()?;

    let keys: Vec<Value> = s
        .keys()
        .map(|k| match k {
            TableKey::Nil => Value::Nil,
            TableKey::Bool(b) => Value::Bool(*b),
            TableKey::Int(i) => Value::Int(*i),
            TableKey::Symbol(sid) => Value::Symbol(*sid),
            TableKey::String(st) => Value::String(st.as_str().into()),
        })
        .collect();

    Ok(crate::value::list(keys))
}

/// Get all values from a struct as a list
/// (struct-values struct)
pub fn prim_struct_values(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("struct-values requires exactly 1 argument (struct)".to_string());
    }

    let s = args[0].as_struct()?;
    let values: Vec<Value> = s.values().cloned().collect();
    Ok(crate::value::list(values))
}

/// Check if a struct has a key
/// (struct-has? struct key)
pub fn prim_struct_has(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("struct-has? requires exactly 2 arguments (struct, key)".to_string());
    }

    let s = args[0].as_struct()?;
    let key = TableKey::from_value(&args[1])?;

    Ok(Value::Bool(s.contains_key(&key)))
}

/// Get the number of entries in a struct
/// (struct-length struct)
pub fn prim_struct_length(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("struct-length requires exactly 1 argument (struct)".to_string());
    }

    let s = args[0].as_struct()?;
    Ok(Value::Int(s.len() as i64))
}
