//! Struct operations primitives (immutable hash maps)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, TableKey, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Declarative table of struct primitives.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "struct",
        func: prim_struct,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(2),
        doc: "Create a new struct without a key (immutable)",
        params: &["struct", "key"],
        category: "struct",
        example: "(struct/del (struct :a 1 :b 2) :a)",
        aliases: &["struct-del"],
    },
    PrimitiveDef {
        name: "freeze",
        func: prim_freeze,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert a mutable collection to its immutable equivalent (table→struct, @set→set)",
        params: &["collection"],
        category: "struct",
        example: "(freeze @{:a 1 :b 2})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "thaw",
        func: prim_thaw,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Convert an immutable collection to its mutable equivalent (struct→table, set→@set)",
        params: &["collection"],
        category: "struct",
        example: "(thaw {:a 1 :b 2})",
        aliases: &[],
    },
];

/// Create an immutable struct from key-value pairs
/// (struct key1 val1 key2 val2 ...)
pub(crate) fn prim_struct(args: &[Value]) -> (SignalBits, Value) {
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

/// Create a new struct without a key (immutable)
/// (struct-del struct key) returns a new struct
pub(crate) fn prim_struct_del(args: &[Value]) -> (SignalBits, Value) {
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

/// Convert a mutable collection to its immutable equivalent
/// (freeze collection) -> immutable collection
/// Handles: table -> struct, @set -> set
pub(crate) fn prim_freeze(args: &[Value]) -> (SignalBits, Value) {
    // Handle mutable set -> immutable set
    if let Some(s) = args[0].as_set_mut() {
        let items: BTreeSet<Value> = s.borrow().iter().copied().collect();
        return (SIG_OK, Value::set(items));
    }
    // Already immutable set — return as-is
    if args[0].is_set() {
        return (SIG_OK, args[0]);
    }
    // Already immutable struct — return as-is
    if args[0].is_struct() {
        return (SIG_OK, args[0]);
    }
    // Handle table -> struct (existing behavior)
    let t = match args[0].as_table() {
        Some(t) => t,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "freeze: expected mutable collection (table, @set), got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let map = t.borrow().clone();
    (SIG_OK, Value::struct_from(map))
}

/// Convert an immutable collection to its mutable equivalent
/// (thaw collection) -> mutable collection
/// Handles: struct -> table, set -> @set
pub(crate) fn prim_thaw(args: &[Value]) -> (SignalBits, Value) {
    // Handle immutable set -> mutable set
    if let Some(s) = args[0].as_set() {
        let items: BTreeSet<Value> = s.iter().copied().collect();
        return (SIG_OK, Value::set_mut(items));
    }
    // Already mutable set — return as-is
    if args[0].is_set_mut() {
        return (SIG_OK, args[0]);
    }
    // Already mutable table — return as-is
    if args[0].is_table() {
        return (SIG_OK, args[0]);
    }
    // Handle struct -> table (existing behavior)
    let s = match args[0].as_struct() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "thaw: expected immutable collection (struct, set), got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let map = s.clone();
    (SIG_OK, Value::table_from(map))
}
