// audited: 2026-09-09
// The mutable-store seam records a content edge for a store the alloc-time
// scan never saw.
//
// docs/impl/region/ownership.md
//
// A post-alloc store into a mutable container adds an edge after the alloc-time
// scan has run, so `value/arena/mutate.rs` records it beside the incref that
// pays for it. Each test here is a counter-factual against a seam that tracks
// the count and not the edge: red while no edge is recorded, green once one is.

use super::*;

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
