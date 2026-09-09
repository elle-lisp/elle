// audited: 2026-09-09
// `deref` checks that a Value's tag agrees with the heap object it points at.
//
// docs/regions.md
//
// The two disagree when an allocation was freed and its address was handed to
// a different variant, so this check is the runtime's nearest thing to a
// use-after-free detector at the point of the read.

use super::*;

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
