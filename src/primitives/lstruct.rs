//! Struct operations primitives (mutable hash tables)
//!
//! Polymorphic collection access (get, put) is in `access.rs`.
use crate::primitives::def::RegionEffect;
use crate::primitives::def::RetType;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{sorted_struct_remove, TableKey, Value};
use std::collections::BTreeMap;

use super::access::prim_get;

// Declarative table of struct primitives.
primitive! {
    "@struct" => prim_struct_mut {
        ret: RetType::MutableStruct,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable struct from key-value pairs",
        category: "struct",
        example: "(@struct :a 1 :b 2)",
        effect: RegionEffect::Fresh,
    }
    "get" => prim_get {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Get a value from a collection (tuple, array, string, struct) by index or key, with optional default",
        params: &["collection", "key", "default"],
        category: "struct",
        example: "(get [1 2 3] 0)",
        effect: RegionEffect::Funnel,
    }
    "keys" => prim_keys {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all keys from a struct as a list",
        params: &["collection"],
        category: "struct",
        example: "(keys (@struct :a 1 :b 2))",
        effect: RegionEffect::Fresh,
    }
    "values" => prim_values {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all values from a struct as a list",
        params: &["collection"],
        category: "struct",
        example: "(values (@struct :a 1 :b 2))",
        effect: RegionEffect::Fresh,
    }
    "has?" => prim_has_key {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if a collection has a key, element, or substring. Works on structs (key lookup), sets (membership), and strings (substring check).",
        params: &["collection", "key-or-value"],
        category: "struct",
        example: "(has? {:a 1} :a) #=> true\n(has? |1 2 3| 2) #=> true\n(has? \"hello\" \"ell\") #=> true",
        aliases: &["has-key?", "contains?"],
        effect: RegionEffect::Mixed,
    }
}

/// Create a mutable struct from key-value pairs
/// (@struct key1 val1 key2 val2 ...)
pub(crate) fn prim_struct_mut(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if !args.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            ctx.error(
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

    (SIG_OK, ctx.struct_mut_from(map))
}

/// Polymorphic del - works on structs and sets
/// For @struct: mutates in-place and returns the struct
/// For struct: returns a new struct without the field (immutable)
/// For sets: delegates to set-specific del
/// `(del collection key)`
pub(crate) fn prim_del(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Delegate to set-specific del for set types
    if args[0].is_set() || args[0].is_set_mut() {
        return crate::primitives::sets::prim_del(ctx, args);
    }

    let key = match TableKey::from_value(&args[1]) {
        Some(k) => k,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "struct keys must be immutable (got {})",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };

    if args[0].is_struct_mut() {
        crate::value::arena::struct_remove_with_decref(ctx.heap_mut(), args[0], &key);
        (SIG_OK, args[0]) // Return the mutated struct
    } else if args[0].is_struct() {
        let s = prim_arg!(ctx, args, 0, as_struct, "del", "struct");
        (
            SIG_OK,
            ctx.struct_from_sorted(sorted_struct_remove(s, &key)),
        ) // Return new struct
    } else {
        type_error!(ctx, args[0], "del", "struct or set")
    }
}

/// Polymorphic keys - works on both structs
/// `(keys collection)`
pub(crate) fn prim_keys(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_struct_mut() {
        let mstruct = prim_arg!(ctx, args, 0, as_struct_mut, "keys", "struct");
        let borrowed = mstruct.borrow();
        let keys: Vec<Value> = borrowed.keys().map(|k| k.to_value(ctx)).collect();
        (SIG_OK, ctx.list(keys))
    } else if args[0].is_struct() {
        let s = prim_arg!(ctx, args, 0, as_struct, "keys", "struct");
        let keys: Vec<Value> = s.iter().map(|(k, _)| k.to_value(ctx)).collect();
        (SIG_OK, ctx.list(keys))
    } else {
        type_error!(ctx, args[0], "keys", "struct")
    }
}

/// Polymorphic values - works on both structs
/// `(values collection)`
pub(crate) fn prim_values(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_struct_mut() {
        let mstruct = prim_arg!(ctx, args, 0, as_struct_mut, "values", "struct");
        let borrowed = mstruct.borrow();
        let values: Vec<Value> = borrowed.values().copied().collect();
        (SIG_OK, ctx.list(values))
    } else if args[0].is_struct() {
        let s = prim_arg!(ctx, args, 0, as_struct, "values", "struct");
        let values: Vec<Value> = s.iter().map(|(_, v)| *v).collect();
        (SIG_OK, ctx.list(values))
    } else {
        type_error!(ctx, args[0], "values", "struct")
    }
}

/// Polymorphic has? - works on structs, sets, and strings
/// `(has? collection key-or-value)`
pub(crate) fn prim_has_key(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0],
        "Collection",
        "has?",
        args,
        ctx,
    )
}
