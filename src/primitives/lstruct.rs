//! Struct operations primitives (mutable hash tables)
//!
//! Polymorphic collection access (get, put) is in `access.rs`.
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::fiberheap;
use crate::value::types::Arity;
use crate::value::{error_val, sorted_struct_contains, sorted_struct_remove, TableKey, Value};
use std::collections::BTreeMap;

use super::access::{prim_get, prim_put};

/// Declarative table of struct primitives.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "@struct",
        func: prim_table,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable struct from key-value pairs",
        params: &[],
        category: "struct",
        example: "(@struct :a 1 :b 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "get",
        func: prim_get,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Get a value from a collection (tuple, array, string, struct) by index or key, with optional default",
        params: &["collection", "key", "default"],
        category: "struct",
        example: "(get [1 2 3] 0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "put",
        func: prim_put,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Put a value into a collection. For immutable structs/arrays/strings: returns a new collection. For mutable @struct/@array/@string: mutates in place and returns the same reference. For sets: (put set value) delegates to add.",
        params: &["collection", "key-or-value", "value"],
        category: "struct",
        example: "(put {:a 1} :b 2) #=> {:a 1 :b 2}\n(put |1 2| 3) #=> |1 2 3|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "del",
        func: prim_del,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Delete a key from a struct or element from a set. For immutable structs: returns a new struct without the key. For mutable @struct: mutates in place and returns the same reference. For sets: delegates to set del.",
        params: &["collection", "key"],
        category: "struct",
        example: "(del (@struct :a 1) :a)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keys",
        func: prim_keys,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all keys from a struct as a list",
        params: &["collection"],
        category: "struct",
        example: "(keys (@struct :a 1 :b 2))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "values",
        func: prim_values,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all values from a struct as a list",
        params: &["collection"],
        category: "struct",
        example: "(values (@struct :a 1 :b 2))",
        aliases: &[],
    },
     PrimitiveDef {
         name: "has?",
         func: prim_has_key,
         signal: Signal::errors(),
         arity: Arity::Exact(2),
         doc: "Check if a collection has a key, element, or substring. Works on structs (key lookup), sets (membership), and strings (substring check).",
         params: &["collection", "key-or-value"],
         category: "struct",
         example: "(has? {:a 1} :a) #=> true\n(has? |1 2 3| 2) #=> true\n(has? \"hello\" \"ell\") #=> true",
         aliases: &["has-key?", "contains?"],
     },
];

/// Create a mutable struct from key-value pairs
/// (@struct key1 val1 key2 val2 ...)
pub(crate) fn prim_table(args: &[Value]) -> (SignalBits, Value) {
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                "@struct: requires an even number of arguments (key-value pairs)".to_string(),
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
        fiberheap::incref(value);
        map.insert(key, value);
    }

    (SIG_OK, Value::struct_mut_from(map))
}

/// Polymorphic del - works on structs and sets
/// For @struct: mutates in-place and returns the struct
/// For struct: returns a new struct without the field (immutable)
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
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
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
        // Decref removed value: it leaves a durable collection reference.
        if let Some(old_val) = mstruct.borrow().get(&key).copied() {
            crate::value::fiberheap::decref_and_free(old_val);
        }
        mstruct.borrow_mut().remove(&key);
        (SIG_OK, args[0]) // Return the mutated struct
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
        (
            SIG_OK,
            Value::struct_from_sorted(sorted_struct_remove(s, &key)),
        ) // Return new struct
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("del: expected struct or set, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Polymorphic keys - works on both structs
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
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
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
        let borrowed = mstruct.borrow();
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
        let keys: Vec<Value> = s.iter().map(|(k, _)| k.to_value()).collect();
        (SIG_OK, crate::value::list(keys))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("keys: expected struct, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Polymorphic values - works on both structs
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
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
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
        let borrowed = mstruct.borrow();
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
        let values: Vec<Value> = s.iter().map(|(_, v)| *v).collect();
        (SIG_OK, crate::value::list(values))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("values: expected struct, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Polymorphic has? - works on structs, sets, and strings
/// `(has? collection key-or-value)`
///
/// For structs: checks if key exists
/// For sets: checks if value is a member
/// For strings: checks if substring is present
pub(crate) fn prim_has_key(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("has?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Delegate to set/string membership check
    if args[0].is_set() || args[0].is_set_mut() || args[0].is_string() || args[0].is_string_mut() {
        return crate::primitives::sets::prim_contains(args);
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
        let mstruct = match args[0].as_struct_mut() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("has?: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        (SIG_OK, Value::bool(mstruct.borrow().contains_key(&key)))
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("has?: expected struct, got {}", args[0].type_name()),
                    ),
                )
            }
        };
        (SIG_OK, Value::bool(sorted_struct_contains(s, &key)))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "has?: expected struct, set, or string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}
