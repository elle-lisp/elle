//! Struct operations primitives (immutable hash maps)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

/// Declarative table of struct primitives.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "struct",
        func: prim_struct,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable struct from key-value pairs",
        params: &[],
        category: "struct",
        example: "(struct :a 1 :b 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "struct/del",
        func: prim_struct_del,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Create a new struct without a key (immutable)",
        params: &["struct", "key"],
        category: "struct",
        example: "(struct/del (struct :a 1 :b 2) :a)",
        aliases: &["struct-del"],
    },
];

/// Create an immutable struct from key-value pairs
/// (struct key1 val1 key2 val2 ...)
pub fn prim_struct(args: &[Value]) -> (SignalBits, Value) {
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "struct: requires an even number of arguments (key-value pairs)".to_string(),
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

    (SIG_OK, Value::struct_from(map))
}

/// Get a value from a struct by key
/// `(struct-get struct key [default])`
pub fn prim_struct_get(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-get: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("struct-get: expected struct, got {}", args[0].type_name()),
                ),
            );
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

    let default = if args.len() == 3 { args[2] } else { Value::NIL };

    (SIG_OK, s.get(&key).copied().unwrap_or(default))
}

/// Create a new struct with an updated key-value pair (immutable)
/// (struct-put struct key value) returns a new struct
pub fn prim_struct_put(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-put: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("struct-put: expected struct, got {}", args[0].type_name()),
                ),
            );
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

    let value = args[2];

    let mut new_map = s.clone();
    new_map.insert(key, value);
    (SIG_OK, Value::struct_from(new_map))
}

/// Create a new struct without a key (immutable)
/// (struct-del struct key) returns a new struct
pub fn prim_struct_del(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-del: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("struct-del: expected struct, got {}", args[0].type_name()),
                ),
            );
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

    let mut new_map = s.clone();
    new_map.remove(&key);
    (SIG_OK, Value::struct_from(new_map))
}

/// Get all keys from a struct as a list
/// (struct-keys struct)
pub fn prim_struct_keys(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-keys: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("struct-keys: expected struct, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let keys: Vec<Value> = s.keys().map(|k| k.to_value()).collect();

    (SIG_OK, crate::value::list(keys))
}

/// Get all values from a struct as a list
/// (struct-values struct)
pub fn prim_struct_values(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-values: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "struct-values: expected struct, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let values: Vec<Value> = s.values().copied().collect();
    (SIG_OK, crate::value::list(values))
}

/// Check if a struct has a key
/// (struct-has? struct key)
pub fn prim_struct_has(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-has?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("struct-has?: expected struct, got {}", args[0].type_name()),
                ),
            );
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

    (SIG_OK, Value::bool(s.contains_key(&key)))
}

/// Get the number of entries in a struct
/// (struct-length struct)
pub fn prim_struct_length(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct-length: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "struct-length: expected struct, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    (SIG_OK, Value::int(s.len() as i64))
}
