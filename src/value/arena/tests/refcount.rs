// audited: 2026-09-09
// The arena's reference-count funnels: what a store, a remove, an escape and a
// move-out each do to the count on a value's region.
//
// docs/impl/region/ownership.md

use super::*;

#[test]
fn region_of_returns_none_for_non_heap() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    assert_eq!(region_of(heap, Value::int(42)), None);
    assert_eq!(region_of(heap, Value::NIL), None);
    assert_eq!(region_of(heap, Value::TRUE), None);
}

#[test]
fn region_of_returns_correct_region() {
    // A runtime allocation classifies as `Some(RuntimeRegion)` — the mortal
    // region it was born in. Every region is mortal.
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    assert!(val.is_heap());
    let heap = unsafe { &mut *heap_ptr };
    assert_eq!(region_of(heap, val), Some(rid));
}

#[test]
fn incref_inserted_element_increfs_region() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let rc_before = region_rc(unsafe { &*heap_ptr }, rid);
    let heap = unsafe { &mut *heap_ptr };
    incref_inserted_element(heap, val);
    assert_eq!(region_rc(unsafe { &*heap_ptr }, rid), rc_before + 1);
    let heap = unsafe { &mut *heap_ptr };
    decref_removed_element(heap, val);
}

#[test]
fn decref_removed_element_decrefs_region() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let heap = unsafe { &mut *heap_ptr };
    incref_inserted_element(heap, val);
    let rc_after_insert = region_rc(unsafe { &*heap_ptr }, rid);
    let heap = unsafe { &mut *heap_ptr };
    decref_removed_element(heap, val);
    assert_eq!(region_rc(unsafe { &*heap_ptr }, rid), rc_after_insert - 1);
}

#[test]
fn rebind_stored_element_same_region_is_noop() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val1, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let val2 = unsafe {
        (*heap_ptr).alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rid)
    };
    let rc_before = region_rc(unsafe { &*heap_ptr }, rid);
    let heap = unsafe { &mut *heap_ptr };
    rebind_stored_element(heap, val1, val2);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid),
        rc_before,
        "same-region store should not change RC"
    );
}

#[test]
fn incref_inserted_element_noop_for_immediates() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    incref_inserted_element(heap, Value::int(42));
    let heap = unsafe { &mut *heap_ptr };
    incref_inserted_element(heap, Value::NIL);
}

#[test]
fn incref_for_escape_raises_rc_like_incref_region() {
    // The Rule 5 escape funnel must be behaviourally identical to a plain
    // `incref_region` — it only adds the audit tag and trace label. If this
    // ever diverges, every escape site silently mis-counts.
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let before = region_rc(unsafe { &*heap_ptr }, rid);
    let heap = unsafe { &mut *heap_ptr };
    incref_for_escape(heap, Some(rid), EscapeSite::NativeCallResult);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid),
        before + 1,
        "escape funnel must incref"
    );
    let heap = unsafe { &mut *heap_ptr };
    decref_region(heap, Some(rid));
    assert_eq!(region_rc(unsafe { &*heap_ptr }, rid), before);
    let _ = val;
}

#[test]
fn incref_for_escape_none_is_noop() {
    // A non-heap value (no region) escaping is a no-op — the funnel's
    // type-level guard (an absent region is `None`).
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let r = region_of(heap, Value::int(42));
    let heap = unsafe { &mut *heap_ptr };
    incref_for_escape(heap, r, EscapeSite::MutableStore);
    let heap = unsafe { &mut *heap_ptr };
    incref_for_escape(heap, None, EscapeSite::TerminalSignal);
}

#[test]
fn mutable_array_push_keeps_region_alive() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid_a) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));

    let heap = unsafe { &mut *heap_ptr };
    let (arr, rid_b) = alloc_in_fresh_region(
        heap,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
            traits: Value::NIL,
        },
    );
    assert_ne!(rid_a, rid_b);

    let rc_before = region_rc(unsafe { &*heap_ptr }, rid_a);
    {
        let mut ctx = crate::primitives::ctx::Alloc::with_region(rid_b, unsafe { &mut *heap_ptr });
        let _ = crate::primitives::seq::seq_push(&arr, val, &mut ctx);
    }
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        rc_before + 1,
        "push should incref val's region"
    );

    // Release the initial owning reference, leaving the @array as val's sole
    // holder (rc 1). This is the shape that exposes the pop UAF: the element the
    // array holds is now sole-owned by the array's stored reference.
    decref_if_present(unsafe { &mut *heap_ptr }, rid_a);
    assert_eq!(region_rc(unsafe { &*heap_ptr }, rid_a), 1);

    let popped = {
        let mut ctx = crate::primitives::ctx::Alloc::with_region(rid_b, unsafe { &mut *heap_ptr });
        crate::primitives::seq::seq_pop(&arr, crate::segment::Generation::NEWEST, &mut ctx)
            .expect("pop of a non-empty @array")
    };
    // `pop` MOVES the last element out to the caller — it does NOT destroy it
    // (unlike `del`/`remove`, which discard the removed value). The @array's
    // stored reference is released, but the RETURNED value carries its own owning
    // reference, so val's region survives the pop (rc stays 1) and the returned
    // Value still points into a LIVE region. Freeing it here (a bare
    // `decref_removed_element` taking rc 1 → 0) is the free-before-retain UAF: the
    // call would hand back a Value into a region it just freed (the `raw-pop`
    // oracle probe; docs/impl/region/ownership.md § "The outgoing edge table").
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        1,
        "pop moves the element out — its region survives, held by the returned value"
    );
    assert_eq!(
        region_of(unsafe { &mut *heap_ptr }, popped),
        Some(rid_a),
        "the popped value still lives in its region (not freed under the returned Value)"
    );

    // The caller releasing the popped value (its `DecrefValueRegion` at the
    // result's decref_point) is what finally frees the region.
    decref_if_present(unsafe { &mut *heap_ptr }, rid_a);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        0,
        "releasing the moved-out value frees its region"
    );
}

#[test]
fn pop_extracts_moved_out_element_from_owned_subtree() {
    // The moves-out-of-Owned-subtree case (region_pop_tail_moves_out_uaf): when the
    // popped element was ADOPTED into its container's Owned subtree — a heap value
    // pushed into a LOCAL Owned @array, where the ownership forest emits an
    // `AdoptRegion` at the push site — `incref`/`decref` on it are inert (RC frozen).
    // So `pop` must EXTRACT it (Owned → Counted(1)); otherwise it stays interior and
    // the container's subtree drop frees it under the returned Value. This pins the
    // extract path beside `mutable_array_push_keeps_region_alive`'s Counted path.
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, rid_a) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));

    let heap = unsafe { &mut *heap_ptr };
    let (arr, rid_b) = alloc_in_fresh_region(
        heap,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
            traits: Value::NIL,
        },
    );
    assert_ne!(rid_a, rid_b);

    // Push val into the @array (records the edge + increfs), drop the initial
    // reference so the array's stored reference is val's sole holder, then ADOPT
    // val's region into the array's Owned subtree — the runtime shape of a heap
    // element pushed into a local Owned @array.
    {
        let mut ctx = crate::primitives::ctx::Alloc::with_region(rid_b, unsafe { &mut *heap_ptr });
        let _ = crate::primitives::seq::seq_push(&arr, val, &mut ctx);
    }
    decref_if_present(unsafe { &mut *heap_ptr }, rid_a);
    let heap = unsafe { &mut *heap_ptr };
    heap.adopt_region(rid_b, rid_a);
    assert!(
        heap.region_is_owned(rid_a),
        "val's region is adopted into the @array's Owned subtree"
    );

    // Pop moves val OUT — the extract must move it back to a caller-owned Counted(1).
    let popped = {
        let mut ctx = crate::primitives::ctx::Alloc::with_region(rid_b, unsafe { &mut *heap_ptr });
        crate::primitives::seq::seq_pop(&arr, crate::segment::Generation::NEWEST, &mut ctx)
            .expect("pop of a non-empty @array")
    };
    let heap = unsafe { &mut *heap_ptr };
    assert!(
        !heap.region_is_owned(rid_a),
        "pop extracts the moved-out element from the container's Owned subtree"
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        1,
        "the extracted element carries the caller's single owning reference"
    );
    assert_eq!(
        region_of(unsafe { &mut *heap_ptr }, popped),
        Some(rid_a),
        "the popped value still lives in its (now Counted) region"
    );

    // Freeing the CONTAINER's subtree must NOT reclaim the extracted element.
    decref_if_present(unsafe { &mut *heap_ptr }, rid_b);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        1,
        "the container's subtree drop no longer reclaims the moved-out element"
    );

    // The caller releasing the popped value is what finally frees it.
    decref_if_present(unsafe { &mut *heap_ptr }, rid_a);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        0,
        "releasing the moved-out value frees its region"
    );
}
