//! Table operations primitives (mutable hash tables)
//!
//! Polymorphic collection access (get, put) is in `access.rs`.
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

use super::access::{prim_get, prim_put};

/// Declarative table of table primitives.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "table",
        func: prim_table,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Get all values from a table or struct as a list",
        params: &["collection"],
        category: "table",
        example: "(values (table :a 1 :b 2))",
        aliases: &[],
    },
     PrimitiveDef {
         name: "has?",
         func: prim_has_key,
         effect: Effect::inert(),
         arity: Arity::Exact(2),
         doc: "Check if a collection has a key or element",
         params: &["collection", "key"],
         category: "table",
         example: "(has? (table :a 1) :a)",
         aliases: &["has-key?"],
     },
];

/// Create a mutable table from key-value pairs
/// (table key1 val1 key2 val2 ...)
pub(crate) fn prim_table(args: &[Value]) -> (SignalBits, Value) {
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

    (SIG_OK, Value::struct_mut_from(map))
}

/// Polymorphic del - works on tables, structs, and sets
/// For tables: mutates in-place and returns the table
/// For structs: returns a new struct without the field (immutable)
/// For sets: delegates to set-specific del
/// `(del collection key)`
pub(crate) fn prim_del(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("del: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Delegate to set-specific del for set types
    if args[0].is_set() || args[0].is_set_mut() {
        return crate::primitives::sets::prim_del(args);
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

    if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
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
                format!(
                    "del: expected table, struct, or set, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Polymorphic keys - works on both tables and structs
/// `(keys collection)`
pub(crate) fn prim_keys(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keys: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
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
pub(crate) fn prim_values(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("values: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
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
pub(crate) fn prim_has_key(args: &[Value]) -> (SignalBits, Value) {
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

    if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
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
