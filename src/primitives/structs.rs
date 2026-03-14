//! Struct operations primitives (immutable hash maps)
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, TableKey, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Declarative table of struct primitives.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "struct",
        func: prim_struct,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable struct from key-value pairs",
        params: &[],
        category: "struct",
        example: "(struct :a 1 :b 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "freeze",
        func: prim_freeze,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a mutable collection to its immutable equivalent. Handles @array, @struct, @set, @string (requires valid UTF-8), @bytes. Returns immutable values as-is.",
        params: &["collection"],
        category: "struct",
        example: "(freeze @{:a 1 :b 2})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "thaw",
        func: prim_thaw,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert an immutable collection to its mutable equivalent. Handles array, struct, set, string, bytes. Returns mutable values as-is.",
        params: &["collection"],
        category: "struct",
        example: "(thaw {:a 1 :b 2})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pairs",
        func: prim_pairs,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Iterate over struct key-value pairs as [key value] arrays.",
        params: &["struct"],
        category: "struct",
        example: "(pairs {:a 1 :b 2})",
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

/// Convert a mutable collection to its immutable equivalent
/// (freeze collection) -> immutable collection
/// Handles: @array -> array, @struct -> struct, @set -> set, @string -> string, @bytes -> bytes
pub(crate) fn prim_freeze(args: &[Value]) -> (SignalBits, Value) {
    // @array → array
    if let Some(a) = args[0].as_array_mut() {
        let elements = a.borrow().clone();
        return (SIG_OK, Value::array(elements));
    }
    if args[0].is_array() {
        return (SIG_OK, args[0]);
    }

    // @struct → struct
    if let Some(t) = args[0].as_struct_mut() {
        let map = t.borrow().clone();
        return (SIG_OK, Value::struct_from(map));
    }
    if args[0].is_struct() {
        return (SIG_OK, args[0]);
    }

    // @set → set
    if let Some(s) = args[0].as_set_mut() {
        let items: BTreeSet<Value> = s.borrow().iter().copied().collect();
        return (SIG_OK, Value::set(items));
    }
    if args[0].is_set() {
        return (SIG_OK, args[0]);
    }

    // @string → string (fallible: requires valid UTF-8)
    if let Some(buf) = args[0].as_string_mut() {
        let bytes = buf.borrow();
        return match std::str::from_utf8(&bytes) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("freeze: @string contains invalid UTF-8: {}", e),
                ),
            ),
        };
    }
    if args[0].is_string() {
        return (SIG_OK, args[0]);
    }

    // @bytes → bytes
    if let Some(b) = args[0].as_bytes_mut() {
        let data = b.borrow().clone();
        return (SIG_OK, Value::bytes(data));
    }
    if args[0].is_bytes() {
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "freeze: expected collection (@array, @struct, @set, @string, @bytes), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Convert an immutable collection to its mutable equivalent
/// (thaw collection) -> mutable collection
/// Handles: array -> @array, struct -> @struct, set -> @set, string -> @string, bytes -> @bytes
pub(crate) fn prim_thaw(args: &[Value]) -> (SignalBits, Value) {
    // array → @array
    if let Some(a) = args[0].as_array() {
        return (SIG_OK, Value::array_mut(a.to_vec()));
    }
    if args[0].is_array_mut() {
        return (SIG_OK, args[0]);
    }

    // struct → @struct
    if let Some(s) = args[0].as_struct() {
        let map = s.clone();
        return (SIG_OK, Value::struct_mut_from(map));
    }
    if args[0].is_struct_mut() {
        return (SIG_OK, args[0]);
    }

    // set → @set
    if let Some(s) = args[0].as_set() {
        let items: BTreeSet<Value> = s.iter().copied().collect();
        return (SIG_OK, Value::set_mut(items));
    }
    if args[0].is_set_mut() {
        return (SIG_OK, args[0]);
    }

    // string → @string
    if let Some(bytes) = args[0].with_string(|s| s.as_bytes().to_vec()) {
        return (SIG_OK, Value::string_mut(bytes));
    }
    if args[0].is_string_mut() {
        return (SIG_OK, args[0]);
    }

    // bytes → @bytes
    if let Some(b) = args[0].as_bytes() {
        return (SIG_OK, Value::bytes_mut(b.to_vec()));
    }
    if args[0].is_bytes_mut() {
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "thaw: expected collection (array, struct, set, string, bytes), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Convert a struct to a list of [key value] pairs
/// (pairs {:a 1 :b 2}) -> ((:a 1) (:b 2))
pub(crate) fn prim_pairs(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pairs: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(map) = args[0].as_struct() {
        let mut result = Value::EMPTY_LIST;
        // Build list in reverse, then we'll reverse the final result
        for (key, value) in map.iter().rev() {
            let key_val = match key {
                TableKey::Nil => Value::NIL,
                TableKey::Bool(b) => Value::bool(*b),
                TableKey::Int(i) => Value::int(*i),
                TableKey::Symbol(sym) => Value::symbol(sym.0),
                TableKey::String(s) => Value::string(s.as_str()),
                TableKey::Keyword(kw) => Value::keyword(kw.as_str()),
                TableKey::Identity(_) => {
                    // Identity keys are opaque; skip them
                    continue;
                }
            };
            let pair = Value::array(vec![key_val, *value]);
            result = Value::cons(pair, result);
        }
        return (SIG_OK, result);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("pairs: expected struct, got {}", args[0].type_name()),
        ),
    )
}
