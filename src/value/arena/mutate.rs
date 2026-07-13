use super::*;

// ── Tracked mutation helpers ────────────────────────────────────────
//
// THE mutable-store funnel (docs/impl/region/rules.md Rule 5, mutable store): the raw
// `RefCell` accessors of the Value-bearing mutable containers are visible
// only inside `value/`, so these helpers are the only way the rest of the
// crate can store a `Value` into (or remove one from) an @array, @struct,
// @set, box, or capture cell. Each pairs the mutation with its region
// tracking; an uncounted container store is a compile error, not a review
// item.
//
// Region tracking is two ledgers, both maintained here: the incoming RC
// (`incref_inserted_element`/`decref_removed_element`/`rebind_stored_element`,
// arena.rs) and the source region's outgoing edge table (`record_store`/
// `unrecord_store` below — docs/impl/region/ownership.md § "The outgoing edge table").
// Co-locating the edge op with its RC op is what keeps the two from drifting; the
// free-time equivalence oracle asserts the result.

/// Record the outgoing content edge `region(container) → region(elem)` for a value
/// entering a mutable container (the add half of the seam). Source/target are
/// resolved here, where the container is in scope; the reserved/self filter lives
/// in `RegionStore::record_outgoing`.
fn record_store(heap: &mut FiberHeap, container: Value, elem: Value) {
    let src = region_of(heap, container);
    let dst = region_of(heap, elem);
    heap.record_outgoing_edge(src, dst);
}

/// Remove the outgoing content edge for a value leaving a mutable container (the
/// remove/overwrite half), co-located with the matching RC decref.
fn unrecord_store(heap: &mut FiberHeap, container: Value, elem: Value) {
    let src = region_of(heap, container);
    let dst = region_of(heap, elem);
    heap.unrecord_outgoing_edge(src, dst);
}

/// Push a value into a mutable @array, tracking the cross-region reference.
/// Panics if `collection` is not an @array.
/// Returns the collection value.
pub fn push_with_incref(heap: &mut FiberHeap, collection: Value, elem: Value) -> Value {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("push_with_incref: expected @array");
    vec_ref.borrow_mut().push(elem);
    incref_inserted_element(heap, elem);
    record_store(heap, collection, elem);
    collection
}

/// Pop a value from a mutable @array, MOVING it out to the caller.
/// Panics if `collection` is not an @array or if it's empty.
/// Returns the popped value.
///
/// Unlike the other remove funnels (`struct_remove`/`set_del`/`remove_at`), which
/// DISCARD the removed value, `pop` hands it back as the call's result — so the
/// moved-out element must survive the container's release, and it takes one of two
/// paths depending on how the element was held.
///
/// **Owned element (adopted).** When the element was stored into a container the
/// ownership forest made Owned, an `AdoptRegion` moved the element into the
/// container's subtree (`Owned`, RC frozen). `incref`/`decref` are then inert
/// (docs/impl/region/ownership.md § "The runtime: a reclamation typestate"), so the
/// escape-retain path below cannot keep the element alive — and, left interior, the
/// element would be reclaimed by the container's subtree drop while the returned
/// Value still points into it (the moves-out-of-owned-subtree UAF the
/// `region_pop_tail_moves_out_uaf` fixture pins). So EXTRACT it: un-record the
/// container's outgoing edge and move the element back to `Counted(1)` — the
/// caller's one owning reference — via `extract_owned_region`. External uniqueness
/// (the adoption precondition) guarantees the container was its only referrer, so
/// one reference is exact.
///
/// **Counted element (not adopted — the common path).** The element carries an
/// ordinary RC. Hold the caller's reference FIRST (the pass-through retain a native
/// result normally receives from `dispatch_native_call`; `%pop`/`pop` declare
/// `moves_out` so dispatch SKIPS its own, applying it twice would leak one region
/// per op), THEN un-record the container's edge co-located with its RC decref:
/// `decref_removed_element` alone would take a sole-owned element's region to rc 0
/// and free it under the returned Value (the free-before-retain UAF the `raw-pop`
/// oracle probe and the `mutable_array_push_keeps_region_alive` unit test pin). The
/// un-record + decref stay paired (the two-ledger co-location invariant), only now
/// the incoming RC never transiently reaches zero.
pub fn pop_with_decref(heap: &mut FiberHeap, collection: Value) -> Value {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("pop_with_decref: expected @array");
    let popped = vec_ref
        .borrow_mut()
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
        // Release the container's reference — un-record the edge co-located with the
        // RC decref. Both resolve `popped`'s region, which the retain above kept live.
        unrecord_store(heap, collection, popped);
        decref_removed_element(heap, popped);
    }
    popped
}

/// Extend a mutable @array with multiple values, tracking each cross-region reference.
/// Panics if `collection` is not an @array.
/// Returns the collection value.
pub fn extend_with_incref(heap: &mut FiberHeap, collection: Value, elems: &[Value]) -> Value {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("extend_with_incref: expected @array");
    vec_ref.borrow_mut().extend_from_slice(elems);
    for &elem in elems {
        incref_inserted_element(heap, elem);
        record_store(heap, collection, elem);
    }
    collection
}

/// Drain the last `n` values from a mutable @array (fewer if shorter),
/// releasing each tracked ref. Returns the removed values in array order.
/// Panics if `collection` is not an @array.
pub fn drain_tail_with_decref(heap: &mut FiberHeap, collection: Value, n: usize) -> Vec<Value> {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("drain_tail_with_decref: expected @array");
    let mut vec = vec_ref.borrow_mut();
    let len = vec.len();
    let removed: Vec<Value> = vec.drain(len - n.min(len)..).collect();
    drop(vec);
    for &v in &removed {
        // Un-record before decref (the decref may free `v`'s region — see `pop`).
        unrecord_store(heap, collection, v);
        decref_removed_element(heap, v);
    }
    removed
}

/// Insert a value at `index` in a mutable @array, tracking the stored ref.
/// Panics if `collection` is not an @array or `index` is out of bounds
/// (mirrors `Vec::insert`).
pub fn insert_with_incref(heap: &mut FiberHeap, collection: Value, index: usize, elem: Value) {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("insert_with_incref: expected @array");
    vec_ref.borrow_mut().insert(index, elem);
    incref_inserted_element(heap, elem);
    record_store(heap, collection, elem);
}

/// Remove and return the value at `index` in a mutable @array, releasing its
/// tracked ref. Panics if `collection` is not an @array or `index` is out of
/// bounds (mirrors `Vec::remove`).
pub fn remove_at_with_decref(heap: &mut FiberHeap, collection: Value, index: usize) -> Value {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("remove_at_with_decref: expected @array");
    let removed = vec_ref.borrow_mut().remove(index);
    // Un-record before decref (the decref may free `removed`'s region — see `pop`).
    unrecord_store(heap, collection, removed);
    decref_removed_element(heap, removed);
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
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("set_at_with_rebind: expected @array");
    let old = std::mem::replace(&mut vec_ref.borrow_mut()[index], new);
    // Un-record the old edge BEFORE the rebind (its decref may free `old`'s region,
    // making `old`'s region unresolvable); record the new edge after (the rebind's
    // incref keeps `new`'s region live).
    unrecord_store(heap, collection, old);
    rebind_stored_element(heap, old, new);
    record_store(heap, collection, new);
    old
}

/// Mutable access to an @array for membership-NEUTRAL operations ONLY —
/// sort/reverse/swap/rotate, where no value enters or leaves the container
/// and region RC is untouched. Storing or removing through this funnel is a
/// Rule-5 violation; the debug length assert is the tripwire for the
/// insert/remove case (an in-place replacement it cannot see — use
/// [`set_at_with_rebind`]).
pub fn with_array_mut_neutral<R>(collection: Value, f: impl FnOnce(&mut Vec<Value>) -> R) -> R {
    let vec_ref = collection
        .as_array_mut_raw()
        .expect("with_array_mut_neutral: expected @array");
    let mut vec = vec_ref.borrow_mut();
    let len_before = vec.len();
    let out = f(&mut vec);
    debug_assert_eq!(
        len_before,
        vec.len(),
        "with_array_mut_neutral: membership changed — use the tracked funnels"
    );
    out
}

/// Insert into a mutable @struct, swapping tracked refs: increfs the stored
/// value, decrefs a displaced one (insert vs replace). Returns the displaced
/// value. Panics if `collection` is not an @struct.
pub fn struct_put_with_rebind(
    heap: &mut FiberHeap,
    collection: Value,
    key: crate::value::heap::TableKey,
    val: Value,
) -> Option<Value> {
    // A heap-valued KEY (fiber/closure/list) is a cross-region reference the struct
    // holds, exactly like the value — the free-time content scan enumerates it
    // (`find_object_cross_refs`'s `LStructMut` arm walks keys). So a NEW key must be
    // increfed and its outgoing edge recorded here, symmetric with the alloc-scan
    // that does it for a struct BUILT with keys (region-struct-heap-key-uaf.lisp);
    // otherwise the recorded edge table and the free scan disagree (a missed
    // store-funnel edge the equivalence oracle detonates on). Captured before
    // `insert` moves the key. A rebind (`Some(old)`) keeps the pre-existing key
    // (BTreeMap::insert does not replace an equal key), so its edge is untouched.
    let mut key_heap_vals: Vec<Value> = Vec::new();
    key.for_each_heap_value(&mut |kv| key_heap_vals.push(*kv));
    let map_ref = collection
        .as_struct_mut_raw()
        .expect("struct_put_with_rebind: expected @struct");
    let old = map_ref.borrow_mut().insert(key, val);
    match old {
        Some(old) => {
            // Un-record old before the rebind's decref may free its region; record
            // the new edge after (see `set_at_with_rebind`).
            unrecord_store(heap, collection, old);
            rebind_stored_element(heap, old, val);
            record_store(heap, collection, val);
        }
        None => {
            incref_inserted_element(heap, val);
            record_store(heap, collection, val);
            for kv in &key_heap_vals {
                incref_inserted_element(heap, *kv);
                record_store(heap, collection, *kv);
            }
        }
    }
    old
}

/// Remove a key from a mutable @struct, releasing the removed value's
/// tracked ref. Returns the removed value. Panics if `collection` is not an
/// @struct.
pub fn struct_remove_with_decref(
    heap: &mut FiberHeap,
    collection: Value,
    key: &crate::value::heap::TableKey,
) -> Option<Value> {
    let map_ref = collection
        .as_struct_mut_raw()
        .expect("struct_remove_with_decref: expected @struct");
    // `remove_entry` hands back the STORED key, not the caller's lookup key — for a
    // value-equal-but-distinct heap key (a list) they are two allocations in two
    // regions, and the edge was recorded against the STORED one (mirrors the @set
    // del fix). Release the stored key's heap edges symmetrically with the incref/
    // record `struct_put_with_rebind` added.
    let removed = map_ref.borrow_mut().remove_entry(key);
    if let Some((stored_key, v)) = removed {
        // Un-record before decref (the decref may free `v`'s region — see `pop`).
        unrecord_store(heap, collection, v);
        decref_removed_element(heap, v);
        stored_key.for_each_heap_value(&mut |kv| {
            unrecord_store(heap, collection, *kv);
            decref_removed_element(heap, *kv);
        });
        return Some(v);
    }
    None
}

/// Insert into a mutable @set, tracking the stored ref — only when actually
/// inserted (a duplicate stores nothing). The element must already be frozen
/// (set invariant; the caller owns freezing). Returns whether it was
/// inserted. Panics if `collection` is not an @set.
pub fn set_add_with_incref(heap: &mut FiberHeap, collection: Value, frozen: Value) -> bool {
    let set_ref = collection
        .as_set_mut_raw()
        .expect("set_add_with_incref: expected @set");
    let inserted = set_ref.borrow_mut().insert(frozen);
    if inserted {
        incref_inserted_element(heap, frozen);
        record_store(heap, collection, frozen);
    }
    inserted
}

/// Remove from a mutable @set, releasing the removed value's tracked ref —
/// only when actually removed. Returns whether it was removed. Panics if
/// `collection` is not an @set.
pub fn set_del_with_decref(heap: &mut FiberHeap, collection: Value, frozen: &Value) -> bool {
    let set_ref = collection
        .as_set_mut_raw()
        .expect("set_del_with_decref: expected @set");
    // `take` HANDS BACK the stored member — a plain `remove` yields only a bool. For
    // a heap member the stored element and the caller's `frozen` are DISTINCT
    // allocations in distinct regions that merely compare equal, and the add half
    // (`set_add_with_incref`) recorded the outgoing edge and incref against the
    // STORED member's region. Resolving the un-record and decref from `frozen`
    // instead un-records an edge that was never recorded (outgoing-edge accounting
    // drift) and decrefs a live region the caller still owns (an over-free). Resolve
    // them from the removed member, mirroring `struct_remove_with_decref` — whose
    // `BTreeMap::remove` already returns the stored value.
    let removed = set_ref.borrow_mut().take(frozen);
    if let Some(member) = removed {
        // Un-record before decref (the decref may free the region — see `pop`).
        unrecord_store(heap, collection, member);
        decref_removed_element(heap, member);
        true
    } else {
        false
    }
}

/// Store into a user box (`(box v)`), swapping the tracked refs (old-region
/// vs new-region rebind). Returns the displaced value. Panics if `bx` is not
/// a box.
pub fn lbox_store_with_rebind(heap: &mut FiberHeap, bx: Value, new: Value) -> Value {
    let cell = bx
        .as_lbox_raw()
        .expect("lbox_store_with_rebind: expected box");
    let old = std::mem::replace(&mut *cell.borrow_mut(), new);
    // Un-record old before the rebind's decref may free its region; record new after
    // (see `set_at_with_rebind`).
    unrecord_store(heap, bx, old);
    rebind_stored_element(heap, old, new);
    record_store(heap, bx, new);
    old
}

/// Store into a compiler capture cell, tracking relative to the CELL's
/// region (Rule 5, capture store): the displaced value's region is released
/// and the stored value's region retained only when they differ from the
/// cell's own region — a same-region store is the self-edge the alloc-scan
/// already filters. Returns the displaced value. Panics if `cell_val` is not
/// a capture cell. The single store routine for every tier (interpreter,
/// JIT, WASM host).
pub fn capture_store_with_rebind(heap: &mut FiberHeap, cell_val: Value, new_value: Value) -> Value {
    let cell_ref = cell_val
        .as_capture_cell_raw()
        .expect("capture_store_with_rebind: expected capture cell");
    let old_value = *cell_ref.borrow();
    if crate::config::get().has_trace("rc") {
        eprintln!(
            "[trace:rc] capture_store cell_r={:?} old_r={:?} new_r={:?}",
            region_of(heap, cell_val),
            region_of(heap, old_value),
            region_of(heap, new_value),
        );
    }
    if let Some(cell_r) = region_of(heap, cell_val) {
        let old_r = region_of(heap, old_value);
        let new_r = region_of(heap, new_value);
        if let Some(old_r) = old_r {
            if old_r != cell_r {
                decref_region(heap, Some(old_r));
                // The cell's outgoing edge to its old contents, removed in lockstep
                // with the RC decref (the source is the CELL's region, not a
                // container — docs/impl/region/ownership.md § "The outgoing edge table").
                heap.unrecord_outgoing_edge(Some(cell_r), Some(old_r));
            }
        }
        if let Some(new_r) = new_r {
            if new_r != cell_r {
                incref_for_escape(heap, Some(new_r), EscapeSite::CaptureStore);
                heap.record_outgoing_edge(Some(cell_r), Some(new_r));
            }
        }
    }
    *cell_ref.borrow_mut() = new_value;
    old_value
}
