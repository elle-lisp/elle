// audited: 2026-10-30
//! The tracked mutable-store funnels: every store into, or removal from, a mutable container.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/ownership.md
//! docs/impl/mutable.md

use super::*;
use crate::value::store::Put;

// ── Tracked mutation helpers ────────────────────────────────────────
//
// THE mutable-store funnel (docs/impl/region/rules.md Rule 5, mutable store).
// A store's writes are visible only inside `value/` (src/value/store.rs). So
// these helpers are the only way the rest of the crate can store a `Value` into
// (or remove one from) an @array, @struct, @set, box, or capture cell. Each
// pairs the mutation with its region tracking; an uncounted container store is
// a compile error, not a review item.
//
// Region tracking is two ledgers, both maintained here: the incoming RC
// (`incref_inserted_element`/`decref_removed_element`/`rebind_stored_element`,
// arena.rs) and the source region's outgoing edge table (`record_store`/
// `unrecord_store` below, the outgoing edge table).
// Co-locating the edge op with its RC op is what keeps the two from drifting; the
// free-time equivalence oracle asserts the result.

/// Record the outgoing content edge `region(container) → region(elem)` for a value
/// entering a mutable container (the add half of the seam). Source/target are
/// resolved here, where the container is in scope, and an immediate `elem` has no
/// edge, so it resolves no container; the reserved/self filter lives in
/// `RegionStore::record_outgoing`.
fn record_store(heap: &mut FiberHeap, container: Value, elem: Value) {
    let Some(dst) = region_of(heap, elem) else {
        return;
    };
    let src = region_of(heap, container);
    heap.record_outgoing_edge(src, Some(dst));
}

/// Remove the outgoing content edge for a value leaving a mutable container (the
/// remove/overwrite half), co-located with the matching RC decref.
fn unrecord_store(heap: &mut FiberHeap, container: Value, elem: Value) {
    let Some(dst) = region_of(heap, elem) else {
        return;
    };
    let src = region_of(heap, container);
    heap.unrecord_outgoing_edge(src, Some(dst));
}

/// [`rebind_stored_element`], with `site` tagging the retain (Rule 5).
pub(super) fn rebind_counted(
    heap: &mut FiberHeap,
    container: Value,
    old: Value,
    new: Value,
    site: EscapeSite,
) {
    let old_r = counted_store_region(heap, container, old);
    let new_r = counted_store_region(heap, container, new);
    if old_r == new_r {
        return;
    }
    incref_for_escape(heap, new_r, site);
    decref_region(heap, old_r);
}

/// The heap `Value`s of `key` that this seam counts: the ones resolving to a
/// region other than `container`'s. A stored key is interned into the
/// container's own region, so its heap value is normally a self-edge, which
/// neither ledger counts — the rule the free-time cascade follows through its
/// `own_id` filter. A `Heap` key is not interned, stays a real cross-region
/// reference, and does come back from here.
///
/// Both @struct funnels resolve their key edges here, because the remove half
/// cannot tell how the key it removes was stored (docs/impl/values.md).
fn counted_key_values(
    heap: &FiberHeap,
    container: Value,
    key: &crate::value::heap::TableKey,
) -> Vec<Value> {
    let mut counted = Vec::new();
    key.for_each_heap_value(&mut |kv| {
        if region_of(heap, *kv) != region_of(heap, container) {
            counted.push(*kv);
        }
    });
    counted
}

/// Push a value into a mutable @array, tracking the cross-region reference.
/// Panics if `collection` is not an @array, or if it must grow and no region
/// of `heap` holds it.
/// Returns the collection value.
pub fn push_with_incref(heap: &mut FiberHeap, collection: Value, elem: Value) -> Value {
    let room = growth_room(heap, collection, "push_with_incref");
    collection
        .array_store()
        .expect("push_with_incref: expected @array")
        .push(elem, room);
    incref_inserted_element(heap, collection, elem);
    record_store(heap, collection, elem);
    collection
}

/// Pop a value from a mutable @array, MOVING it out to the caller.
/// Panics if `collection` is not an @array or if it's empty.
/// Returns the popped value.
///
/// The other remove funnels (`struct_remove`/`set_del`/`remove_at`) DISCARD the
/// removed value. `pop` hands it back as the call's result, so the moved-out
/// element must survive the container's release. It takes one of two paths,
/// depending on how the element was held.
///
/// **Owned element (adopted).** When the element was stored into a container the
/// ownership forest made Owned, an `AdoptRegion` moved the element into the
/// container's subtree (`Owned`, RC frozen). `incref`/`decref` are then inert
/// (docs/impl/region/ownership.md), so the escape-retain path below cannot keep the
/// element alive. Left interior, the element would be reclaimed by the container's
/// subtree drop while the returned Value still points into it (pinned by the
/// `region_pop_tail_moves_out_uaf` fixture). So EXTRACT it. First un-record the
/// container's outgoing edge, so it is not counted. Then move the element back to
/// `Counted` via `extract_owned_region`. Its count is the caller's one owning
/// reference plus any remaining recorded external edges, each held by a live
/// container that releases it through the ordinary cascade.
///
/// **Counted element (not adopted — the common path).** The element carries an
/// ordinary RC. Hold the caller's reference FIRST: the pass-through retain a native
/// result normally receives from `dispatch_native_call`. `%pop`/`pop` declare
/// `moves_out`, so dispatch SKIPS its own; applying it twice would leak one region
/// per op. THEN un-record the container's edge co-located with its RC decref.
/// `decref_removed_element` alone would take a sole-owned element's region to rc 0
/// and free it under the returned Value (pinned by the `raw-pop` oracle probe and
/// the `mutable_array_push_keeps_region_alive` unit test). The un-record + decref
/// stay paired (the two-ledger co-location invariant), and the incoming RC never
/// transiently reaches zero.
pub fn pop_with_decref(heap: &mut FiberHeap, collection: Value) -> Value {
    let popped = collection
        .array_store()
        .expect("pop_with_decref: expected @array")
        .pop()
        .expect("pop_with_decref: empty @array");
    let popped_region = region_of(heap, popped);
    if popped_region.is_some_and(|r| heap.region_is_owned(r)) {
        // The element leaves the container's Owned subtree: drop the container's
        // edge to it, then move it to a caller-owned `Counted(1)`.
        unrecord_store(heap, collection, popped);
        if let Some(r) = popped_region {
            heap.extract_owned_region(r);
        }
    } else {
        // Hold the caller's reference first (the popped value is the call result),
        // so the region survives the container's release below.
        incref_for_escape(heap, popped_region, EscapeSite::NativeCallResult);
        // Release the container's reference, un-recording the edge beside the RC
        // decref. Both resolve `popped`'s region, which the retain above kept live.
        // An element in the container's own region was never counted, so neither
        // ledger moves for it, and the caller's retain is the reference it holds.
        unrecord_store(heap, collection, popped);
        decref_removed_element(heap, collection, popped);
    }
    popped
}

/// Extend a mutable @array with multiple values, tracking each cross-region reference.
/// Panics if `collection` is not an @array, or if it must grow and no region
/// of `heap` holds it.
/// Returns the collection value.
pub fn extend_with_incref(heap: &mut FiberHeap, collection: Value, elems: &[Value]) -> Value {
    let room = growth_room(heap, collection, "extend_with_incref");
    collection
        .array_store()
        .expect("extend_with_incref: expected @array")
        .extend(elems, room);
    for &elem in elems {
        incref_inserted_element(heap, collection, elem);
        record_store(heap, collection, elem);
    }
    collection
}

/// Drain the last `n` values from a mutable @array (fewer if shorter),
/// releasing each tracked ref. Returns the removed values in array order.
/// Panics if `collection` is not an @array.
pub fn drain_tail_with_decref(heap: &mut FiberHeap, collection: Value, n: usize) -> Vec<Value> {
    let removed = collection
        .array_store()
        .expect("drain_tail_with_decref: expected @array")
        .drain_tail(n);
    for &v in &removed {
        // Un-record before decref (the decref may free `v`'s region — see `pop`).
        unrecord_store(heap, collection, v);
        decref_removed_element(heap, collection, v);
    }
    removed
}

/// Insert a value at `index` in a mutable @array, tracking the stored ref.
/// Panics if `collection` is not an @array, if `index` is out of bounds
/// (mirrors `Vec::insert`), or if it must grow and no region of `heap` holds it.
pub fn insert_with_incref(heap: &mut FiberHeap, collection: Value, index: usize, elem: Value) {
    let room = growth_room(heap, collection, "insert_with_incref");
    collection
        .array_store()
        .expect("insert_with_incref: expected @array")
        .insert(index, elem, room);
    incref_inserted_element(heap, collection, elem);
    record_store(heap, collection, elem);
}

/// Remove and return the value at `index` in a mutable @array, releasing its
/// tracked ref. Panics if `collection` is not an @array or `index` is out of
/// bounds (mirrors `Vec::remove`).
pub fn remove_at_with_decref(heap: &mut FiberHeap, collection: Value, index: usize) -> Value {
    let removed = collection
        .array_store()
        .expect("remove_at_with_decref: expected @array")
        .remove(index);
    // Un-record before decref (the decref may free `removed`'s region — see `pop`).
    unrecord_store(heap, collection, removed);
    decref_removed_element(heap, collection, removed);
    removed
}

/// Replace the value at `index` in a mutable @array, swapping the tracked
/// refs (store increfs, overwrite decrefs). Returns the displaced value.
/// Panics if `collection` is not an @array or `index` is out of bounds.
pub fn set_at_with_rebind(
    heap: &mut FiberHeap,
    collection: Value,
    index: usize,
    new: Value,
) -> Value {
    let old = collection
        .array_store()
        .expect("set_at_with_rebind: expected @array")
        .replace(index, new);
    // Un-record the old edge BEFORE the rebind (its decref may free `old`'s region,
    // making `old`'s region unresolvable); record the new edge after (the rebind's
    // incref keeps `new`'s region live).
    unrecord_store(heap, collection, old);
    rebind_stored_element(heap, collection, old, new);
    record_store(heap, collection, new);
    old
}

/// Mutable access to an @array for membership-NEUTRAL operations ONLY —
/// sort/reverse/swap/rotate, where no value enters or leaves the container
/// and region RC is untouched. A slice cannot change its length, so this
/// cannot insert or remove. Storing a new value into a slot through it is a
/// Rule-5 violation the slice cannot refuse: use [`set_at_with_rebind`].
pub fn with_array_mut_neutral<R>(collection: Value, f: impl FnOnce(&mut [Value]) -> R) -> R {
    collection
        .array_store()
        .expect("with_array_mut_neutral: expected @array")
        .with_slice_mut(f)
}

/// Insert into a mutable @struct, swapping tracked refs: increfs the stored
/// value, decrefs a displaced one (insert vs replace). Returns the displaced
/// value. Panics if `collection` is not an @struct, or if a new key must grow
/// it and no region of `heap` holds it.
pub fn struct_put_with_rebind(
    heap: &mut FiberHeap,
    collection: Value,
    key: crate::value::heap::TableKey,
    val: Value,
) -> Option<Value> {
    // The store is probed with the BORROWED key. A rebind keeps the stored
    // key, so no key edge moves, and interning a key it does not store would
    // leave the copy unreachable in the container's region. The caller's key
    // borrows whatever it was built from (`TableKey::from_value`), so a new key
    // is interned into the container's own region before it is stored, as the
    // immutable constructors do (docs/impl/values.md). A key that holds no heap
    // value has nothing to intern, so it resolves no region.
    let store = collection
        .struct_store()
        .expect("struct_put_with_rebind: expected @struct");
    let h = &mut *heap;
    let put = store.put(&key, val, move || {
        let key = match key.heap_value().and_then(|_| region_of(h, collection)) {
            Some(r) => key.intern_into(h, r),
            None => key,
        };
        (key, growth_room(h, collection, "struct_put_with_rebind"))
    });
    match put {
        Put::Edited(old) => {
            // Un-record old before the rebind's decref may free its region;
            // record the new edge after (see `set_at_with_rebind`).
            unrecord_store(heap, collection, old);
            rebind_stored_element(heap, collection, old, val);
            record_store(heap, collection, val);
            Some(old)
        }
        Put::Stored(key) => {
            // A heap-valued KEY the struct holds outside its own region is a
            // reference like the value's, and the free-time content scan
            // enumerates it (`find_object_cross_refs`'s `LStructMut` arm walks
            // keys). A new one is increfed and its edge recorded here, as the
            // alloc-scan does for a struct BUILT with keys
            // (region-struct-heap-key-uaf.lisp). Otherwise the recorded edge
            // table and the free scan disagree: a missed store-funnel edge the
            // equivalence oracle detonates on. `counted_key_values` decides
            // WHICH of the key's values that is.
            let key_heap_vals = counted_key_values(heap, collection, &key);
            incref_inserted_element(heap, collection, val);
            record_store(heap, collection, val);
            for kv in &key_heap_vals {
                incref_inserted_element(heap, collection, *kv);
                record_store(heap, collection, *kv);
            }
            None
        }
    }
}

/// Remove a key from a mutable @struct, releasing the removed value's
/// tracked ref. Returns the removed value. Panics if `collection` is not an
/// @struct.
pub fn struct_remove_with_decref(
    heap: &mut FiberHeap,
    collection: Value,
    key: &crate::value::heap::TableKey,
) -> Option<Value> {
    let store = collection
        .struct_store()
        .expect("struct_remove_with_decref: expected @struct");
    // `remove_entry` hands back the STORED key, not the caller's lookup key. For a
    // value-equal-but-distinct heap key (a list) they are two allocations in two
    // regions, and the edge was recorded against the STORED one, as
    // `set_del_with_decref` resolves the stored member. Its heap edges are released
    // through `counted_key_values`, which is also what `struct_put_with_rebind`
    // takes them on. This half cannot tell whether the key it removes was interned
    // by a constructor or by that funnel, so the two must agree on which of a key's
    // values is a reference at all.
    let removed = store.remove_entry(key);
    if let Some((stored_key, v)) = removed {
        // Every region this call will release is resolved FIRST, and every
        // un-record runs before any decref: a decref may free a region, and a
        // freed page can no longer answer `region_of` (see `pop`).
        let key_heap_vals = counted_key_values(heap, collection, &stored_key);
        unrecord_store(heap, collection, v);
        for kv in &key_heap_vals {
            unrecord_store(heap, collection, *kv);
        }
        decref_removed_element(heap, collection, v);
        for kv in key_heap_vals {
            decref_removed_element(heap, collection, kv);
        }
        return Some(v);
    }
    None
}

/// Insert into a mutable @set, tracking the stored ref — only when actually
/// inserted (a duplicate stores nothing). The element must already be frozen
/// (set invariant; the caller owns freezing). Returns whether it was
/// inserted. Panics if `collection` is not an @set, or if it must grow and no
/// region of `heap` holds it.
pub fn set_add_with_incref(heap: &mut FiberHeap, collection: Value, frozen: Value) -> bool {
    let room = growth_room(heap, collection, "set_add_with_incref");
    let inserted = collection
        .set_store()
        .expect("set_add_with_incref: expected @set")
        .insert(frozen, room);
    if inserted {
        incref_inserted_element(heap, collection, frozen);
        record_store(heap, collection, frozen);
    }
    inserted
}

/// Remove from a mutable @set, releasing the removed value's tracked ref —
/// only when actually removed. Returns whether it was removed. Panics if
/// `collection` is not an @set.
pub fn set_del_with_decref(heap: &mut FiberHeap, collection: Value, frozen: &Value) -> bool {
    let store = collection
        .set_store()
        .expect("set_del_with_decref: expected @set");
    // `take` HANDS BACK the stored member — a plain `remove` yields only a bool. For
    // a heap member, the stored element and the caller's `frozen` are DISTINCT
    // allocations in distinct regions that merely compare equal. The add half
    // (`set_add_with_incref`) recorded the outgoing edge and incref against the
    // STORED member's region. Resolving the un-record and decref from `frozen`
    // instead un-records an edge that was never recorded (outgoing-edge accounting
    // drift) and decrefs a live region the caller still owns (an over-free). Resolve
    // them from the removed member, as `struct_remove_with_decref` does with the
    // entry `remove_entry` returns.
    let removed = store.take(frozen);
    if let Some(member) = removed {
        // Un-record before decref (the decref may free the region — see `pop`).
        unrecord_store(heap, collection, member);
        decref_removed_element(heap, collection, member);
        true
    } else {
        false
    }
}

/// Store into a user box (`(box v)`), swapping the tracked refs (old-region
/// vs new-region rebind). Returns the displaced value. Panics if `bx` is not
/// a box.
pub fn lbox_store_with_rebind(heap: &mut FiberHeap, bx: Value, new: Value) -> Value {
    let old = bx
        .lbox_store()
        .expect("lbox_store_with_rebind: expected box")
        .replace(new);
    // Un-record old before the rebind's decref may free its region; record new after
    // (see `set_at_with_rebind`).
    unrecord_store(heap, bx, old);
    rebind_stored_element(heap, bx, old, new);
    record_store(heap, bx, new);
    old
}

/// Store into a compiler capture cell, tracking relative to the CELL's
/// region (Rule 5, capture store). The stored value's region is retained
/// before the displaced value's is released, and neither moves when the two
/// are one region, or when either is the cell's own. A cell in no region of
/// `heap` tracks nothing. Returns the
/// displaced value. Panics if `cell_val` is not a capture cell. The single
/// store routine for every tier (interpreter, JIT, WASM host).
pub fn capture_store_with_rebind(heap: &mut FiberHeap, cell_val: Value, new_value: Value) -> Value {
    let store = cell_val
        .capture_cell_store()
        .expect("capture_store_with_rebind: expected capture cell");
    let old_value = store.get();
    if crate::config::get().has_trace("rc") {
        eprintln!(
            "[trace:rc] capture_store cell_r={:?} old_r={:?} new_r={:?}",
            region_of(heap, cell_val),
            region_of(heap, old_value),
            region_of(heap, new_value),
        );
    }
    if region_of(heap, cell_val).is_some() {
        // Un-record old before the rebind's decref may free its region; record new
        // after (see `set_at_with_rebind`).
        unrecord_store(heap, cell_val, old_value);
        rebind_counted(
            heap,
            cell_val,
            old_value,
            new_value,
            EscapeSite::CaptureStore,
        );
        record_store(heap, cell_val, new_value);
    }
    store.replace(new_value);
    old_value
}
