//! audited: 2026-09-17
//! The keyed reads over a struct, a `@struct` and a subprocess, plus the
//! `@struct` constructor.
//!
//! Those reads are `keys`, `values`, `has?`, and the body behind `del`.
//!
//! `get` is registered here and implemented in src/primitives/access.rs, where
//! the indexed collections share its arms. `del` reaches this file's body
//! through the `%del` intrinsics; `put` is a stdlib closure and no primitive.
//!
//! docs/structs.md
use crate::primitives::def::RegionEffect;
use crate::primitives::def::RetType;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{sorted_struct_remove, TableKey, Value};
use std::collections::BTreeMap;

use super::access::prim_get;
use crate::io::request::ProcessHandle;
use crate::primitives::subprocess::{subprocess_read, SUBPROCESS_KEYS};

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
        doc: "Get a value from a collection (tuple, array, string, struct, subprocess) by index or key, with optional default",
        params: &["collection", "key", "default"],
        category: "struct",
        example: "(get [1 2 3] 0)",
        effect: RegionEffect::Funnel,
    }
    "keys" => prim_keys {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all keys from a struct, or a subprocess's closed key set, as a list",
        params: &["collection"],
        category: "struct",
        example: "(keys (@struct :a 1 :b 2))",
        effect: RegionEffect::Fresh,
    }
    "values" => prim_values {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all values from a struct, or a subprocess's, as a list",
        params: &["collection"],
        category: "struct",
        example: "(values (@struct :a 1 :b 2))",
        effect: RegionEffect::Fresh,
    }
    "has?" => prim_has_key {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if a collection has a key, element, or substring. Works on structs and subprocesses (key lookup), sets (membership), and strings (substring check).",
        params: &["collection", "key-or-value"],
        category: "struct",
        example: "(has? {:a 1} :a) #=> true\n(has? |1 2 3| 2) #=> true\n(has? \"hello\" \"ell\") #=> true",
        aliases: &["has-key?", "contains?"],
        // A read-only trait dispatcher: the result is unbounded (a `with-traits`
        // override of `:Collection` may return anything, so neither `Immediate`
        // nor `Fresh` holds on every path) while the store side is bounded (the
        // built-in method reads and returns a bool; a user closure stores only
        // through the runtime-counted funnel). Unbounded result + no store is
        // exactly `Opaque` — no arg clique (docs/impl/region/effects.md § Opaque;
        // tests/elle/region-has-clique-leak.lisp).
        effect: RegionEffect::Opaque,
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

/// Polymorphic del — structs and sets. A subprocess is read-only and refuses.
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

/// Polymorphic keys — structs, and a subprocess's closed key set.
/// `(keys collection)`
pub(crate) fn prim_keys(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_struct_mut() {
        let mstruct = prim_arg!(ctx, args, 0, as_struct_mut, "keys", "struct");
        let borrowed = mstruct.borrow();
        let keys: Vec<Value> = borrowed.keys().map(|k| k.to_value()).collect();
        (SIG_OK, ctx.list(keys))
    } else if args[0].is_struct() {
        let s = prim_arg!(ctx, args, 0, as_struct, "keys", "struct");
        let keys: Vec<Value> = s.iter().map(|(k, _)| k.to_value()).collect();
        (SIG_OK, ctx.list(keys))
    } else if args[0].as_external::<ProcessHandle>().is_some() {
        let keys: Vec<Value> = SUBPROCESS_KEYS.iter().map(|k| Value::keyword(k)).collect();
        (SIG_OK, ctx.list(keys))
    } else {
        type_error!(ctx, args[0], "keys", "struct or subprocess")
    }
}

/// Polymorphic values — structs, and a subprocess's reads in key order.
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
    } else if let Some(handle) = args[0].as_external::<ProcessHandle>() {
        // Read through the same function `get` uses, in the same order `keys`
        // reports, so the three cannot drift apart.
        let values: Vec<Value> = SUBPROCESS_KEYS
            .iter()
            .filter_map(|k| subprocess_read(handle, Value::keyword(k)))
            .collect();
        (SIG_OK, ctx.list(values))
    } else {
        type_error!(ctx, args[0], "values", "struct or subprocess")
    }
}

/// Polymorphic has? — structs, subprocesses, sets, and strings.
/// `(has? collection key-or-value)`
pub(crate) fn prim_has_key(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // A pre-check, as the other non-traited shapes take (docs/traits.md § Edge
    // cases). An external carries no trait table, so dispatch would refuse a
    // subprocess before reaching a method — and the answer here is a key-set
    // membership test, which no `with-traits` override is meant to replace.
    if let Some(handle) = args[0].as_external::<ProcessHandle>() {
        return (
            SIG_OK,
            Value::bool(subprocess_read(handle, args[1]).is_some()),
        );
    }
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0],
        "Collection",
        "has?",
        args,
        ctx,
    )
}
