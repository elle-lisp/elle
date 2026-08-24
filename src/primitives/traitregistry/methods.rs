use super::*;

use crate::primitives::def::{PrimitiveDef, RegionEffect};

/// Build the :Sequence method struct (immutable) into `heap`'s root region.
pub(super) fn build_sequence_methods(heap: &mut FiberHeap) -> Value {
    use crate::signals::Signal;
    use crate::value::types::Arity;

    // Native function definitions for sequence methods — `&'static`
    // native-fn handles, carried as immediate `prim_id` values (no region).
    primitive!(static SEQ_FIRST: PrimitiveDef = "trait:Sequence:first" => trait_seq_first {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Sequence trait: first element", params: &["self"],
        category: "trait", effect: RegionEffect::Funnel,
    });
    primitive!(static SEQ_REST: PrimitiveDef = "trait:Sequence:rest" => trait_seq_rest {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Sequence trait: rest of sequence", params: &["self"],
        category: "trait", effect: RegionEffect::Funnel,
    });
    primitive!(static SEQ_LAST: PrimitiveDef = "trait:Sequence:last" => trait_seq_last {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Sequence trait: last element", params: &["self"],
        category: "trait", effect: RegionEffect::Funnel,
    });
    primitive!(static SEQ_NTH: PrimitiveDef = "trait:Sequence:nth" => trait_seq_nth {
        signal: Signal::errors(), arity: Arity::Exact(2),
        doc: "Sequence trait: nth element", params: &["self", "n"],
        category: "trait", effect: RegionEffect::Funnel,
    });
    primitive!(static SEQ_ITER: PrimitiveDef = "trait:Sequence:iter" => trait_seq_iter {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Sequence trait: fiber iterator", params: &["self"],
        category: "trait", effect: RegionEffect::Fresh,
        ret: crate::primitives::def::RetType::Fiber,
    });

    let mut entries = BTreeMap::new();
    entries.insert(
        TableKey::Keyword("first".into()),
        Value::native_fn(&SEQ_FIRST),
    );
    entries.insert(
        TableKey::Keyword("rest".into()),
        Value::native_fn(&SEQ_REST),
    );
    entries.insert(
        TableKey::Keyword("last".into()),
        Value::native_fn(&SEQ_LAST),
    );
    entries.insert(TableKey::Keyword("nth".into()), Value::native_fn(&SEQ_NTH));
    entries.insert(
        TableKey::Keyword("iter".into()),
        Value::native_fn(&SEQ_ITER),
    );
    alloc_trait_table(heap, entries)
}

/// Build the :Collection method struct (immutable) into `heap`'s root region.
///
/// All collection types currently share the same native implementations
/// (coll_len, coll_empty, coll_has dispatch internally on type). If
/// collection types ever need divergent method sets, split this back
/// into per-category builders.
pub(super) fn build_collection_methods(heap: &mut FiberHeap) -> Value {
    use crate::signals::Signal;
    use crate::value::types::Arity;

    primitive!(static COLL_LENGTH: PrimitiveDef = "trait:Collection:length" => trait_coll_length {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Collection trait: element count", params: &["self"],
        category: "trait", effect: RegionEffect::Immediate,
    });
    primitive!(static COLL_EMPTY: PrimitiveDef = "trait:Collection:empty?" => trait_coll_empty {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Collection trait: is empty?", params: &["self"],
        category: "trait", effect: RegionEffect::Immediate,
    });
    primitive!(static COLL_HAS: PrimitiveDef = "trait:Collection:has?" => trait_coll_has {
        signal: Signal::errors(), arity: Arity::Exact(2),
        doc: "Collection trait: membership test", params: &["self", "needle"],
        category: "trait", effect: RegionEffect::Immediate,
    });
    primitive!(static COLL_CONJ: PrimitiveDef = "trait:Collection:conj" => trait_coll_conj {
        signal: Signal::errors(), arity: Arity::Exact(2),
        doc: "Collection trait: add element", params: &["self", "item"],
        category: "trait", effect: RegionEffect::Funnel,
    });
    primitive!(static COLL_EMPTY_NEW: PrimitiveDef = "trait:Collection:empty" => trait_coll_empty_new {
        signal: Signal::errors(), arity: Arity::Exact(1),
        doc: "Collection trait: empty container of same type", params: &["self"],
        category: "trait", effect: RegionEffect::Fresh,
    });

    let mut entries = BTreeMap::new();
    entries.insert(
        TableKey::Keyword("length".into()),
        Value::native_fn(&COLL_LENGTH),
    );
    entries.insert(
        TableKey::Keyword("empty?".into()),
        Value::native_fn(&COLL_EMPTY),
    );
    entries.insert(
        TableKey::Keyword("has?".into()),
        Value::native_fn(&COLL_HAS),
    );
    entries.insert(
        TableKey::Keyword("conj".into()),
        Value::native_fn(&COLL_CONJ),
    );
    entries.insert(
        TableKey::Keyword("empty".into()),
        Value::native_fn(&COLL_EMPTY_NEW),
    );
    alloc_trait_table(heap, entries)
}

/// Allocate an immutable trait-table struct into `heap`'s pinned root region.
///
/// Uses [`alloc_root`](crate::value::arena::alloc_root). The instance's registry
/// holds the table for the lifetime of *this instance* — pinned alive by
/// reference count in an ordinary, reclaimable region, not forever: the teardown
/// sweep releases the root region by RC (`teardown_process_root_regions`), so the
/// table is reclaimed when the instance ends.
pub(super) fn alloc_trait_table(heap: &mut FiberHeap, entries: BTreeMap<TableKey, Value>) -> Value {
    use crate::value::heap::{alloc_root, HeapObject};
    let sorted: Vec<(TableKey, Value)> = entries.into_iter().collect();
    alloc_root(
        heap,
        HeapObject::LStruct {
            data: sorted,
            traits: Value::NIL,
        },
    )
}

/// Allocate a mutable trait-table struct as a process-lifetime ROOT of `heap`
/// (see [`alloc_trait_table`]).
pub(super) fn alloc_trait_table_mut(
    heap: &mut FiberHeap,
    entries: BTreeMap<TableKey, Value>,
) -> Value {
    use crate::value::heap::{alloc_root, HeapObject};
    use std::cell::RefCell;
    use std::rc::Rc;
    alloc_root(
        heap,
        HeapObject::LStructMut {
            data: Rc::new(RefCell::new(entries)),
            traits: Value::NIL,
        },
    )
}

/// Build a traitset @struct from optional protocol method structs into `heap`.
pub(super) fn make_traitset(
    heap: &mut FiberHeap,
    sequence: Option<Value>,
    collection: Option<Value>,
) -> Value {
    let mut entries = BTreeMap::new();
    if let Some(seq) = sequence {
        entries.insert(TableKey::Keyword("Sequence".into()), seq);
    }
    if let Some(coll) = collection {
        entries.insert(TableKey::Keyword("Collection".into()), coll);
    }
    alloc_trait_table_mut(heap, entries)
}

// ── Trait method implementations ────────────────────────────────────

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};

pub(super) fn trait_seq_first(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::seq::seq_first(&args[0], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_seq_rest(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::seq::seq_rest(&args[0], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_seq_last(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::seq::seq_last(&args[0], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_seq_nth(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let n = prim_arg!(ctx, args, 1, as_int, "nth", "integer index");
    match super::super::seq::seq_nth(&args[0], n, ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_seq_iter(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::value::fiber::Fiber;

    let val = args[0];

    // Collect all elements, then create a native iterator fiber
    // that yields them one by one on each resume.
    let elements = match super::super::collection::coll_to_vec(&val, ctx) {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, e),
    };

    let mask = crate::signals::SIG_YIELD;
    let fiber = Fiber::native_iter(elements, mask);
    (SIG_OK, ctx.fiber(fiber))
}

pub(super) fn trait_coll_length(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::collection::coll_len(&args[0], ctx) {
        Ok(n) => (SIG_OK, Value::int(n as i64)),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_coll_empty(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::collection::coll_empty(&args[0], ctx) {
        Ok(empty) => (SIG_OK, Value::bool(empty)),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_coll_has(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match super::super::collection::coll_has(&args[0], &args[1], ctx) {
        Ok(found) => (SIG_OK, Value::bool(found)),
        Err(e) => (SIG_ERROR, e),
    }
}

pub(super) fn trait_coll_conj(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let coll = &args[0];
    let item = args[1];

    // Array: append
    if let Some(elems) = coll.as_array() {
        let mut new = elems.to_vec();
        new.push(item);
        return (SIG_OK, ctx.array(new));
    }
    if coll.is_array_mut() {
        // Rule 5 mutable store: the funnel pairs the push with the
        // element-region incref (pinned by region-conj-store.lisp).
        return (
            SIG_OK,
            crate::value::arena::push_with_incref(ctx.heap_mut(), *coll, item),
        );
    }

    // List: prepend (cons)
    if coll.is_pair() || coll.is_empty_list() {
        return (SIG_OK, ctx.pair(item, *coll));
    }

    // Set: add
    if let Some(s) = coll.as_set() {
        let frozen = super::super::sets::freeze_value(item, ctx);
        let mut new: std::collections::BTreeSet<Value> = s.iter().copied().collect();
        new.insert(frozen);
        return (SIG_OK, ctx.set(new));
    }
    if coll.is_set_mut() {
        let frozen = super::super::sets::freeze_value(item, ctx);
        // Rule 5 mutable store: the funnel increfs only when actually
        // inserted (pinned by region-conj-store.lisp).
        crate::value::arena::set_add_with_incref(ctx.heap_mut(), *coll, frozen);
        return (SIG_OK, *coll);
    }

    // String: append string
    if coll.is_string() {
        let s = item.with_string(|s| s.to_string()).unwrap_or_default();
        return coll
            .with_string(|base| {
                let mut new = base.to_string();
                new.push_str(&s);
                (SIG_OK, ctx.string(new))
            })
            .unwrap_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error("type-error", "conj: unreachable string case"),
                )
            });
    }

    // Bytes: append byte
    if let Some(b) = coll.as_bytes() {
        if let Some(n) = item.as_int() {
            if (0..=255).contains(&n) {
                let mut new = b.to_vec();
                new.push(n as u8);
                return (SIG_OK, ctx.bytes(new));
            }
        }
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("conj: unsupported collection type {}", coll.type_name()),
        ),
    )
}

pub(super) fn trait_coll_empty_new(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let coll = &args[0];

    if coll.as_array().is_some() {
        return (SIG_OK, ctx.array(vec![]));
    }
    if coll.is_array_mut() {
        return (SIG_OK, ctx.array_mut(vec![]));
    }
    if coll.is_pair() || coll.is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }
    if coll.as_set().is_some() {
        return (SIG_OK, ctx.set(std::collections::BTreeSet::new()));
    }
    if coll.is_set_mut() {
        return (SIG_OK, ctx.set_mut(std::collections::BTreeSet::new()));
    }
    if coll.as_struct().is_some() {
        return (SIG_OK, ctx.struct_from(std::collections::BTreeMap::new()));
    }
    if coll.is_struct_mut() {
        return (SIG_OK, ctx.struct_mut());
    }
    if coll.is_string() {
        return (SIG_OK, ctx.string(""));
    }
    if coll.as_string_mut().is_some() {
        return (SIG_OK, ctx.string_mut(vec![]));
    }
    if coll.as_bytes().is_some() {
        return (SIG_OK, ctx.bytes(vec![]));
    }
    if coll.as_bytes_mut().is_some() {
        return (SIG_OK, ctx.bytes_mut(vec![]));
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("empty: unsupported collection type {}", coll.type_name()),
        ),
    )
}

// ── Dispatch helper ─────────────────────────────────────────────────
