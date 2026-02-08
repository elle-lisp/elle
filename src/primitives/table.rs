//! Table operations primitives (mutable hash tables)
use crate::value::{TableKey, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Create a mutable table from key-value pairs
/// (table key1 val1 key2 val2 ...)
pub fn prim_table(args: &[Value]) -> Result<Value, String> {
    if !args.len().is_multiple_of(2) {
        return Err("table requires an even number of arguments (key-value pairs)".to_string());
    }

    let mut map = BTreeMap::new();
    for i in (0..args.len()).step_by(2) {
        let key = TableKey::from_value(&args[i])?;
        let value = args[i + 1].clone();
        map.insert(key, value);
    }

    Ok(Value::Table(Rc::new(RefCell::new(map))))
}

/// Get a value from a table by key
/// `(get table key [default])`
pub fn prim_table_get(args: &[Value]) -> Result<Value, String> {
    if args.len() < 2 || args.len() > 3 {
        return Err("get requires 2 or 3 arguments (table, key, [default])".to_string());
    }

    let table = args[0].as_table()?;
    let key = TableKey::from_value(&args[1])?;
    let default = if args.len() == 3 {
        args[2].clone()
    } else {
        Value::Nil
    };

    let borrowed = table.borrow();
    Ok(borrowed.get(&key).cloned().unwrap_or(default))
}

/// Put a key-value pair into a table (mutable, in-place)
/// (put table key value)
pub fn prim_table_put(args: &[Value]) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("put requires exactly 3 arguments (table, key, value)".to_string());
    }

    let table = args[0].as_table()?;
    let key = TableKey::from_value(&args[1])?;
    let value = args[2].clone();

    table.borrow_mut().insert(key, value);
    Ok(args[0].clone()) // Return the table itself
}

/// Delete a key from a table (mutable, in-place)
/// (del table key)
pub fn prim_table_del(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("del requires exactly 2 arguments (table, key)".to_string());
    }

    let table = args[0].as_table()?;
    let key = TableKey::from_value(&args[1])?;

    table.borrow_mut().remove(&key);
    Ok(args[0].clone()) // Return the table itself
}

/// Get all keys from a table as a list
/// (keys table)
pub fn prim_table_keys(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("keys requires exactly 1 argument (table)".to_string());
    }

    let table = args[0].as_table()?;
    let borrowed = table.borrow();

    let keys: Vec<Value> = borrowed
        .keys()
        .map(|k| match k {
            TableKey::Nil => Value::Nil,
            TableKey::Bool(b) => Value::Bool(*b),
            TableKey::Int(i) => Value::Int(*i),
            TableKey::Symbol(sid) => Value::Symbol(*sid),
            TableKey::String(s) => Value::String(s.as_str().into()),
        })
        .collect();

    Ok(crate::value::list(keys))
}

/// Get all values from a table as a list
/// (values table)
pub fn prim_table_values(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("values requires exactly 1 argument (table)".to_string());
    }

    let table = args[0].as_table()?;
    let borrowed = table.borrow();

    let values: Vec<Value> = borrowed.values().cloned().collect();
    Ok(crate::value::list(values))
}

/// Check if a table has a key
/// (has-key? table key)
pub fn prim_table_has(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("has-key? requires exactly 2 arguments (table, key)".to_string());
    }

    let table = args[0].as_table()?;
    let key = TableKey::from_value(&args[1])?;

    Ok(Value::Bool(table.borrow().contains_key(&key)))
}

/// Get the number of entries in a table
/// (length table)
pub fn prim_table_length(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("length requires exactly 1 argument (table)".to_string());
    }

    let table = args[0].as_table()?;
    Ok(Value::Int(table.borrow().len() as i64))
}
