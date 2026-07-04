//! Struct operations primitives (immutable hash maps)
use crate::primitives::def::RegionEffect;
use crate::primitives::def::RetType;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{TableKey, Value};
use std::collections::{BTreeMap, BTreeSet};

// Declarative table of struct primitives.
primitive! {
    "struct" => prim_struct {
        ret: RetType::Struct,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable struct from key-value pairs",
        category: "struct",
        example: "(struct :a 1 :b 2)",
        effect: RegionEffect::Fresh,
    }
    "freeze" => prim_freeze {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a mutable collection to its immutable equivalent. Handles @array, @struct, @set, @string (requires valid UTF-8), @bytes. Returns immutable values as-is.",
        params: &["collection"],
        category: "struct",
        example: "(freeze @{:a 1 :b 2})",
        effect: RegionEffect::Funnel,
    }
    "deep-freeze" => prim_deep_freeze {
        arity: Arity::Exact(1),
        doc: "Recursively freeze a value and all its contents. Converts mutable collections to immutable and recurses into elements. Atoms and non-collection types are returned as-is.",
        params: &["value"],
        category: "struct",
        example: "(deep-freeze @[@[1 2] @{:a 3}])",
        effect: RegionEffect::Funnel,
    }
    "thaw" => prim_thaw {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert an immutable collection to its mutable equivalent. Handles array, struct, set, string, bytes. Returns mutable values as-is.",
        params: &["collection"],
        category: "struct",
        example: "(thaw {:a 1 :b 2})",
        effect: RegionEffect::Funnel,
    }
    "pairs" => prim_pairs {
        arity: Arity::Exact(1),
        doc: "Iterate over struct key-value pairs as [key value] arrays.",
        params: &["struct"],
        category: "struct",
        example: "(pairs {:a 1 :b 2})",
        effect: RegionEffect::Fresh,
    }
}

/// Create an immutable struct from key-value pairs
/// (struct key1 val1 key2 val2 ...)
pub(crate) fn prim_struct(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
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
                    ctx.error(
                        "type-error",
                        format!(
                            "struct keys must be immutable (got {})",
                            args[i].type_name()
                        ),
                    ),
                )
            }
        };
        let value = args[i + 1];
        map.insert(key, value);
    }

    (SIG_OK, ctx.struct_from(map))
}

/// Convert a mutable collection to its immutable equivalent
/// (freeze collection) -> immutable collection
/// Handles: @array -> array, @struct -> struct, @set -> set, @string -> string, @bytes -> bytes
pub(crate) fn prim_freeze(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // @array → array
    if let Some(a) = args[0].as_array_mut() {
        let elements = a.borrow().clone();
        return (SIG_OK, ctx.array(elements));
    }
    if args[0].is_array() {
        return (SIG_OK, args[0]);
    }

    // @struct → struct
    if let Some(t) = args[0].as_struct_mut() {
        let map = t.borrow().clone();
        return (SIG_OK, ctx.struct_from(map));
    }
    if args[0].is_struct() {
        return (SIG_OK, args[0]);
    }

    // @set → set
    if let Some(s) = args[0].as_set_mut() {
        let items: BTreeSet<Value> = s.borrow().iter().copied().collect();
        return (SIG_OK, ctx.set(items));
    }
    if args[0].is_set() {
        return (SIG_OK, args[0]);
    }

    // @string → string (fallible: requires valid UTF-8)
    if let Some(buf) = args[0].as_string_mut() {
        let bytes = buf.borrow();
        return match std::str::from_utf8(&bytes) {
            Ok(s) => (SIG_OK, ctx.string(s)),
            Err(e) => (
                SIG_ERROR,
                ctx.error(
                    "encoding-error",
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
        return (SIG_OK, ctx.bytes(data));
    }
    if args[0].is_bytes() {
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "freeze: expected collection (@array, @struct, @set, @string, @bytes), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Recursively freeze a value and all nested contents.
fn deep_freeze_val(ctx: &mut crate::primitives::ctx::NativeCtx<'_>, val: Value) -> Value {
    // @array → freeze to array, deep-freeze each element
    if let Some(a) = val.as_array_mut() {
        let elements: Vec<Value> = a
            .borrow()
            .iter()
            .map(|v| deep_freeze_val(ctx, *v))
            .collect();
        return ctx.array(elements);
    }
    // immutable array → deep-freeze each element
    if let Some(a) = val.as_array() {
        let elements: Vec<Value> = a.iter().map(|v| deep_freeze_val(ctx, *v)).collect();
        return ctx.array(elements);
    }

    // @struct → freeze to struct, deep-freeze each value
    if let Some(t) = val.as_struct_mut() {
        let map: BTreeMap<TableKey, Value> = t
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), deep_freeze_val(ctx, *v)))
            .collect();
        return ctx.struct_from(map);
    }
    // immutable struct → deep-freeze each value
    if let Some(t) = val.as_struct() {
        let entries: Vec<(TableKey, Value)> = t
            .iter()
            .map(|(k, v)| (k.clone(), deep_freeze_val(ctx, *v)))
            .collect();
        return ctx.struct_from_sorted(entries);
    }

    // @set → freeze to set, deep-freeze each element
    if let Some(s) = val.as_set_mut() {
        let items: BTreeSet<Value> = s
            .borrow()
            .iter()
            .map(|v| deep_freeze_val(ctx, *v))
            .collect();
        return ctx.set(items);
    }
    // immutable set → deep-freeze each element
    if let Some(s) = val.as_set() {
        let items: BTreeSet<Value> = s.iter().map(|v| deep_freeze_val(ctx, *v)).collect();
        return ctx.set(items);
    }

    // cons → deep-freeze car and cdr, rebuild
    if let Some(c) = val.as_pair() {
        let first = deep_freeze_val(ctx, c.first);
        let rest = deep_freeze_val(ctx, c.rest);
        return ctx.pair(first, rest);
    }

    // @string → string
    if let Some(buf) = val.as_string_mut() {
        let bytes = buf.borrow();
        return match std::str::from_utf8(&bytes) {
            Ok(s) => ctx.string(s),
            Err(_) => val, // return as-is if invalid UTF-8
        };
    }

    // @bytes → bytes
    if let Some(b) = val.as_bytes_mut() {
        let data = b.borrow().clone();
        return ctx.bytes(data);
    }

    // atoms and everything else → return as-is
    val
}

/// deep-freeze: recursively freeze a value
pub(crate) fn prim_deep_freeze(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, deep_freeze_val(ctx, args[0]))
}

/// Convert an immutable collection to its mutable equivalent
/// (thaw collection) -> mutable collection
/// Handles: array -> @array, struct -> @struct, set -> @set, string -> @string, bytes -> @bytes
pub(crate) fn prim_thaw(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // array → @array
    if let Some(a) = args[0].as_array() {
        return (SIG_OK, ctx.array_mut(a.to_vec()));
    }
    if args[0].is_array_mut() {
        return (SIG_OK, args[0]);
    }

    // struct → @struct
    if let Some(s) = args[0].as_struct() {
        let map: BTreeMap<_, _> = s.iter().map(|(k, v)| (k.clone(), *v)).collect();
        return (SIG_OK, ctx.struct_mut_from(map));
    }
    if args[0].is_struct_mut() {
        return (SIG_OK, args[0]);
    }

    // set → @set
    if let Some(s) = args[0].as_set() {
        let items: BTreeSet<Value> = s.iter().copied().collect();
        return (SIG_OK, ctx.set_mut(items));
    }
    if args[0].is_set_mut() {
        return (SIG_OK, args[0]);
    }

    // string → @string
    if let Some(bytes) = args[0].with_string(|s| s.as_bytes().to_vec()) {
        return (SIG_OK, ctx.string_mut(bytes));
    }
    if args[0].is_string_mut() {
        return (SIG_OK, args[0]);
    }

    // bytes → @bytes
    if let Some(b) = args[0].as_bytes() {
        return (SIG_OK, ctx.bytes_mut(b.to_vec()));
    }
    if args[0].is_bytes_mut() {
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        ctx.error(
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
pub(crate) fn prim_pairs(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Helper: convert a slice of (TableKey, Value) pairs into a list of [key value] pairs
    fn pairs_from_slice(
        ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
        entries: &[(TableKey, Value)],
    ) -> Value {
        let mut result = Value::EMPTY_LIST;
        for (key, value) in entries.iter().rev() {
            // `to_value` is the single source of truth for TableKey → Value
            // (docs/impl/region-ctx.md): born in the call's region via `ctx`.
            let key_val = key.to_value(ctx);
            let pair = ctx.array(vec![key_val, *value]);
            result = ctx.pair(pair, result);
        }
        result
    }

    if let Some(entries) = args[0].as_struct() {
        return (SIG_OK, pairs_from_slice(ctx, entries));
    }

    if let Some(map) = args[0].as_struct_mut() {
        let borrowed = map.borrow();
        let entries: Vec<(TableKey, Value)> =
            borrowed.iter().map(|(k, v)| (k.clone(), *v)).collect();
        return (SIG_OK, pairs_from_slice(ctx, &entries));
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "pairs: expected struct or @struct, got {}",
                args[0].type_name()
            ),
        ),
    )
}
