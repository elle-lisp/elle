// audited: 2026-11-25
//! The `%`-intrinsic bodies over data: the type predicates, the collection reads and writes, and freeze and thaw.
//!
//! docs/intrinsics.md

use super::*;

/// Each type predicate's body: its name, the name its operand takes, and the
/// test it makes.
macro_rules! predicates {
    ($($name:ident |$v:ident| $test:expr;)*) => {$(
        pub(super) fn $name(
            _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
            args: &[Value],
        ) -> (SignalBits, Value) {
            let $v = &args[0];
            (SIG_OK, Value::bool($test))
        }
    )*};
}

predicates! {
    prim_nil_q |v| v.is_nil();
    prim_empty_q |v| v.is_empty_list();
    prim_bool_q |v| v.is_bool();
    prim_int_q |v| v.is_int();
    prim_float_q |v| v.is_float();
    prim_string_q |v| v.is_string() || v.is_string_mut();
    prim_keyword_q |v| v.is_keyword();
    prim_symbol_q |v| v.is_symbol();
    prim_pair_q |v| v.is_pair();
    prim_array_q |v| v.is_array() || v.is_array_mut();
    prim_struct_q |v| v.is_struct() || v.is_struct_mut();
    prim_set_q |v| v.is_set() || v.is_set_mut();
    prim_bytes_q |v| v.is_bytes() || v.is_bytes_mut();
    prim_box_q |v| v.is_lbox();
    prim_closure_q |v| v.is_closure();
    prim_fiber_q |v| v.is_fiber();
}

pub(super) fn prim_type_of(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::keyword(args[0].type_name()))
}

// ── Data access ─────────────────────────────────────────────────────

pub(super) fn prim_length(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let val = &args[0];
    let len = if val.is_empty_list() || val.is_nil() {
        0
    } else if val.is_pair() {
        match val.list_to_vec() {
            Ok(v) => v.len(),
            Err(_) => return (SIG_ERROR, ctx.error("type-error", "%length: improper list")),
        }
    } else if let Some(a) = val.as_array() {
        a.len()
    } else if let Some(a) = val.array_mut_read() {
        a.len()
    } else if let Some(s) = val.as_struct() {
        s.len()
    } else if let Some(s) = val.struct_mut_read() {
        s.len()
    } else if let Some(s) = val.as_set() {
        s.len()
    } else if let Some(s) = val.set_mut_read() {
        s.len()
    } else if let Some(b) = val.as_bytes() {
        b.len()
    } else if let Some(b) = val.bytes_mut_read() {
        b.len()
    } else if let Some(r) =
        val.with_string(|s| crate::segment::grapheme_count(s, ctx.unicode_generation()))
    {
        r
    } else if let Some(b) = val.string_mut_read() {
        match std::str::from_utf8(&b) {
            Ok(s) => crate::segment::grapheme_count(s, ctx.unicode_generation()),
            Err(_) => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "%length: @string invalid UTF-8"),
                )
            }
        }
    } else {
        return type_err("%length", "collection or string", val, ctx);
    };
    (SIG_OK, Value::int(len as i64))
}

pub(super) fn prim_get(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::access::prim_get(ctx, args)
}

pub(super) fn prim_put(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::access::prim_put(ctx, args)
}

pub(super) fn prim_del(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::lstruct::prim_del(ctx, args)
}

/// The `%add-set`/`%add-set-mut` runtime body — the set-add funnel. Delegates to
/// the polymorphic `sets::prim_add` (which freezes the element and dispatches on
/// the container's runtime mutability: a mutable `@set` stores in place through the
/// arena funnel, an immutable set returns a fresh copy). The monomorphic twins
/// differ only in their static `Set`/`MutableSet` return type — the same
/// shared-body pattern as `prim_put`. Named `_set` to avoid the
/// arithmetic `%add` (`num::prim_add`).
pub(super) fn prim_add_set(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::sets::prim_add(ctx, args)
}

pub(super) fn prim_has(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::lstruct::prim_has_key(ctx, args)
}

// `pub(crate)`: the JIT helper (`elle_jit_push`) and the interpreter intrinsic
// handler (`handle_intr_push`) both run this one body via `run_alloc_intrinsic`,
// so the two tiers cannot drift.
pub(crate) fn prim_push(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let collection = &args[0];
    let value = args[1];
    if collection.is_array_mut() {
        (
            SIG_OK,
            crate::value::arena::push_with_incref(ctx.heap_mut(), *collection, value),
        )
    } else if let Some(elems) = collection.as_array() {
        let mut new = elems.to_vec();
        new.push(value);
        (SIG_OK, ctx.array(new))
    } else {
        type_err("%array-push", "array", collection, ctx)
    }
}

/// The `%push-array` body: an immutable array alone, so its result is fresh on
/// every path, as its signature declares.
pub(super) fn prim_push_array(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_array() {
        prim_push(ctx, args)
    } else {
        type_err("%push-array", "immutable array", &args[0], ctx)
    }
}

/// The `%push-array-mut` body: an `@array` alone, so it answers that `@array`
/// on every path, as its signature declares.
pub(super) fn prim_push_array_mut(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_array_mut() {
        prim_push(ctx, args)
    } else {
        type_err("%push-array-mut", "@array", &args[0], ctx)
    }
}

pub(super) fn prim_pop(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(empty) = args[0].array_mut_read().map(|arr| arr.is_empty()) else {
        return type_err("%pop", "@array", &args[0], ctx);
    };
    if empty {
        // Popping an empty container is an argument error, not a type error — the
        // container's type is fine, its length is not. Aligned with the other two
        // pop-empty paths (`seq::mutate::seq_pop`, `vm::types::intrinsic`) and the
        // `pop empty @array` error-keyword pin in `tests/lang/errors.lisp`.
        return (SIG_ERROR, ctx.error("argument-error", "pop: empty @array"));
    }
    (
        SIG_OK,
        crate::value::arena::pop_with_decref(ctx.heap_mut(), args[0]),
    )
}

pub(crate) fn prim_string_push(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let collection = &args[0];
    let value = args[1];
    // The pushed value may be an immutable string OR a mutable @string. Read
    // its bytes into an owned buffer first. A single bulk append then works:
    // string concat == UTF-8 byte concat, the linear path core.lisp's push-all
    // uses. And no read of an @string is live across the append to
    // `collection`, which may be the same @string.
    let s: String = if let Some(s) = value.with_string(|s| s.to_string()) {
        s
    } else if let Some(buf) = value.string_mut_read() {
        // @string is maintained as valid UTF-8 by its constructors/mutators.
        match String::from_utf8(buf.to_vec()) {
            Ok(s) => s,
            Err(_) => return type_err("%string-push value", "string", &value, ctx),
        }
    } else {
        return type_err("%string-push value", "string", &value, ctx);
    };
    if collection.is_string_mut() {
        crate::value::arena::bytes_append(ctx.heap_mut(), *collection, s.as_bytes());
        (SIG_OK, *collection)
    } else if collection.is_string() {
        let new = collection
            .with_string(|base| {
                let mut r = base.to_string();
                r.push_str(&s);
                ctx.string(r)
            })
            .unwrap();
        (SIG_OK, new)
    } else {
        type_err("%string-push", "string", collection, ctx)
    }
}

pub(crate) fn prim_bytes_push(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let collection = &args[0];
    let value = args[1];
    // The pushed value is either a single byte (integer) OR a whole bytes/@bytes
    // value appended in bulk: the linear binary path core.lisp's push-all uses.
    // It mirrors %string-push bulk-appending a string (string concat == UTF-8
    // byte concat). Read the source into an owned buffer FIRST (like
    // prim_string_push) so no read of an @bytes is live across the append to
    // `collection`, which may be the same @bytes (for example `(append b b)`).
    let src: Vec<u8> = if let Some(i) = value.as_int() {
        vec![i as u8]
    } else if let Some(data) = value.as_bytes() {
        data.to_vec()
    } else if let Some(buf) = value.bytes_mut_read() {
        buf.to_vec()
    } else {
        return type_err("%bytes-push value", "integer or bytes", &value, ctx);
    };
    if collection.is_bytes_mut() {
        crate::value::arena::bytes_append(ctx.heap_mut(), *collection, &src);
        (SIG_OK, *collection)
    } else if let Some(data) = collection.as_bytes() {
        let mut new = data.to_vec();
        new.extend_from_slice(&src);
        (SIG_OK, ctx.bytes(new))
    } else {
        type_err("%bytes-push", "bytes", collection, ctx)
    }
}

// ── Mutability ──────────────────────────────────────────────────────

/// The `%freeze` body: a mutable container alone, so its result is a fresh
/// immutable copy on every path, as its signature declares. The native `freeze`
/// also takes an immutable value, and answers it itself.
pub(crate) fn prim_freeze(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let val = &args[0];
    let result = if let Some(a) = val.array_mut_read() {
        ctx.array(a.to_vec())
    } else if let Some(t) = val.struct_mut_read() {
        let entries: Vec<_> = t.iter().map(|(k, v)| (*k, *v)).collect();
        ctx.struct_from_sorted(entries)
    } else if let Some(s) = val.set_mut_read() {
        ctx.set(s.iter().copied().collect())
    } else if let Some(b) = val.string_mut_read() {
        match std::str::from_utf8(&b) {
            Ok(s) => ctx.string(s),
            Err(_) => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "%freeze: @string invalid UTF-8"),
                )
            }
        }
    } else if let Some(b) = val.bytes_mut_read() {
        ctx.bytes(b.to_vec())
    } else {
        return type_err("%freeze", "mutable container", val, ctx);
    };
    (SIG_OK, result)
}

/// The `%thaw` body: an immutable container alone, so its result is a fresh
/// mutable copy on every path. The native `thaw` also takes a mutable value.
pub(crate) fn prim_thaw(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let val = &args[0];
    let result = if let Some(a) = val.as_array() {
        ctx.array_mut(a)
    } else if let Some(s) = val.as_struct() {
        let entries: std::collections::BTreeMap<_, _> = s.iter().map(|(k, v)| (*k, *v)).collect();
        ctx.struct_mut_from(entries)
    } else if let Some(s) = val.as_set() {
        ctx.set_mut(s.iter().cloned().collect())
    } else if let Some(r) = val.with_string(|s| ctx.string_mut(s.as_bytes().to_vec())) {
        r
    } else if let Some(b) = val.as_bytes() {
        ctx.bytes_mut(b.to_vec())
    } else {
        return type_err("%thaw", "immutable container", val, ctx);
    };
    (SIG_OK, result)
}

// ── Identity ────────────────────────────────────────────────────────

pub(super) fn prim_identical(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].tag == args[1].tag && args[0].payload == args[1].payload),
    )
}
