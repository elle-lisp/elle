use super::*;
use crate::value::heap::{HeapObject, Pair};

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

// ── The outgoing edge table: the mutable-store seam ──────────────────────────
//
// docs/impl/region/ownership.md § "The outgoing edge table". A post-alloc store into a
// mutable container is a content edge added after the alloc-time scan, so the
// mutable-store seam (`value/arena/mutate.rs`) records it co-located with the RC
// incref/decref. Each pin is a counterfactual against the pre-step-0 seam (RC
// tracking only) — RED while the seam records no edge, GREEN once it does.

/// A mutable push records the content edge `B → A` (B = the array's region,
/// A = the pushed value's). RED before the seam records.
#[test]
fn mutable_push_records_outgoing_edge() {
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
    assert!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) }.is_empty(),
        "no edge before the push"
    );
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::push_with_incref(heap, arr, val);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_a.get(), 1)],
        "a mutable push records the content edge B → A"
    );
}

/// Per-reference counts: the same cross-region value pushed twice records count 2;
/// each pop un-records one, the edge vanishing at zero. RED before the seam records.
#[test]
fn duplicate_edges_counted_then_decremented() {
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
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::push_with_incref(heap, arr, val);
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::push_with_incref(heap, arr, val);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_a.get(), 2)],
        "two references to the same target are counted"
    );
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::pop_with_decref(heap, arr);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_a.get(), 1)],
        "a pop un-records one reference"
    );
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::pop_with_decref(heap, arr);
    assert!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) }.is_empty(),
        "the last pop removes the edge"
    );
}

/// An overwrite un-records the old target and records the new: replacing a stored
/// value from region A with one from region C moves the edge `B → A` to `B → C`.
/// RED before the seam records.
#[test]
fn overwrite_removes_old_edge() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val_a, rid_a) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    let heap = unsafe { &mut *heap_ptr };
    let (val_c, rid_c) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    let heap = unsafe { &mut *heap_ptr };
    let (arr, rid_b) = alloc_in_fresh_region(
        heap,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
            traits: Value::NIL,
        },
    );
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::push_with_incref(heap, arr, val_a);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_a.get(), 1)],
        "B → A before the overwrite"
    );
    let heap = unsafe { &mut *heap_ptr };
    crate::value::arena::set_at_with_rebind(heap, arr, 0, val_c);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_c.get(), 1)],
        "an overwrite un-records the old target (A) and records the new (C)"
    );
    let _ = rid_a;
}

/// A mutable-set del of a HEAP member un-records and decrefs the STORED member's
/// region, not the caller's lookup value. The stored member and the lookup are
/// distinct allocations in distinct regions that merely compare equal (set
/// membership is by value); resolving the un-record/decref from the lookup drifts
/// the outgoing-edge table (a plain `BTreeSet::remove` hands back no element) and
/// over-frees the caller's live region. RED against the pre-`take` seam, which
/// un-recorded `region(lookup)` — an edge never recorded.
#[test]
fn set_del_releases_stored_member_not_lookup_value() {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    // The stored member — region A.
    let heap = unsafe { &mut *heap_ptr };
    let (member, rid_a) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    // A distinct, structurally-equal lookup value — region C.
    let heap = unsafe { &mut *heap_ptr };
    let (lookup, rid_c) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    assert_ne!(rid_a, rid_c);
    // The set — region B.
    let heap = unsafe { &mut *heap_ptr };
    let (set, rid_b) = alloc_in_fresh_region(
        heap,
        HeapObject::LSetMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeSet::new())),
            traits: Value::NIL,
        },
    );
    // Add the member: records B → A, increfs A.
    let heap = unsafe { &mut *heap_ptr };
    assert!(crate::value::arena::set_add_with_incref(heap, set, member));
    let rc_a_stored = region_rc(unsafe { &*heap_ptr }, rid_a);
    let rc_c_before = region_rc(unsafe { &*heap_ptr }, rid_c);
    assert_eq!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) },
        vec![(rid_a.get(), 1)],
        "add records the edge B → A (the stored member's region)"
    );
    // Del by the distinct-but-equal lookup value.
    let heap = unsafe { &mut *heap_ptr };
    assert!(
        crate::value::arena::set_del_with_decref(heap, set, &lookup),
        "the value-equal member is found and removed"
    );
    assert!(
        unsafe { (*heap_ptr).outgoing_edges(rid_b) }.is_empty(),
        "del un-records the STORED member's edge B → A"
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_a),
        rc_a_stored - 1,
        "del decrefs the stored member's region"
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, rid_c),
        rc_c_before,
        "del does NOT touch the caller's lookup-value region"
    );
}

#[test]
fn deref_accepts_consistent_tag_and_object() {
    // Sanity: a Value constructed via the safe constructor has
    // matching tag/object — deref does not panic.
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (val, _rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let obj = unsafe { deref(val) };
    assert!(matches!(obj, HeapObject::Pair(_)));
}

#[test]
#[should_panic(expected = "tag/object mismatch")]
fn deref_panics_on_tag_object_mismatch() {
    // Construct a Value whose tag bits disagree with the heap
    // object's discriminant by reaching in directly. This is the
    // canonical signature of a use-after-free: the original
    // allocation was freed and the same address was repurposed
    // for a different HeapObject variant; the stale Value still
    // carries the original tag.
    //
    // Caught telemetry.lisp's bug — type_name returned "@struct"
    // (heap object IS LStructMut) but is_struct_mut() returned
    // false (Value's tag bits said TAG_STRUCT, immutable). The
    // assertion fires at the deref boundary, closest to the
    // observable symptom, so the mismatch is caught at the deref
    // site rather than chased down by hand.
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let (good, _rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    // Swap the tag to something other than TAG_CONS to induce
    // mismatch. TAG_STRUCT (14) is the symptom from telemetry.
    let bad = Value {
        tag: crate::value::repr::TAG_STRUCT,
        payload: good.payload,
    };
    let _ = unsafe { deref(bad) };
}
