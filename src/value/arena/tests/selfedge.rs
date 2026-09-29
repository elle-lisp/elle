// audited: 2026-09-29
// A value stored into a container in its own region is counted by neither half
// of the store funnel.
//
// docs/impl/region/rules.md
//
// Each test builds a container and a value in ONE region, whose count is the one
// reference the test holds. The counter-factual: a funnel that counts the store
// raises the count to 2, and nothing lowers it again, because the free cascade
// skips a self-edge too. The region then outlives its last holder.
//
// The removal tests pin the other half. Fixing only the store would leave a
// remove lowering a count no store raised, which frees the region under its
// holder.

use super::*;
use crate::value::heap::TableKey;
use std::cell::RefCell;
use std::rc::Rc;

fn array_in(heap: &mut FiberHeap, r: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LArrayMut {
            data: Rc::new(RefCell::new(vec![])),
            traits: Value::NIL,
        },
        r,
    )
}

fn struct_in(heap: &mut FiberHeap, r: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LStructMut {
            data: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            traits: Value::NIL,
        },
        r,
    )
}

fn set_in(heap: &mut FiberHeap, r: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LSetMut {
            data: Rc::new(RefCell::new(std::collections::BTreeSet::new())),
            traits: Value::NIL,
        },
        r,
    )
}

fn box_in(heap: &mut FiberHeap, r: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LBox {
            cell: Rc::new(RefCell::new(Value::NIL)),
            traits: Value::NIL,
        },
        r,
    )
}

fn pair_in(heap: &mut FiberHeap, r: RuntimeRegion) -> Value {
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r)
}

/// A fresh heap and one region on it, holding nothing yet.
fn one_region() -> (&'static mut FiberHeap, RuntimeRegion) {
    let heap = unsafe { &mut *crate::value::arena::leaked_test_heap() };
    let r = heap.new_runtime_region();
    (heap, r)
}

/// The region's count is still the test's one reference, and releasing that
/// reference frees it.
fn assert_counted_once_then_freed(heap: &mut FiberHeap, r: RuntimeRegion, what: &str) {
    assert_eq!(
        heap.region_rc(r),
        1,
        "{what}: a store into a container in the value's own region raised the count",
    );
    heap.decref_region(r);
    assert_eq!(
        heap.region_rc(r),
        0,
        "{what}: the last release frees the region"
    );
}

#[test]
fn a_push_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    let v = pair_in(heap, r);
    push_with_incref(heap, arr, v);
    assert_counted_once_then_freed(heap, r, "push");
}

#[test]
fn an_extend_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    let a = pair_in(heap, r);
    let b = pair_in(heap, r);
    extend_with_incref(heap, arr, &[a, b]);
    assert_counted_once_then_freed(heap, r, "extend");
}

#[test]
fn an_insert_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    let v = pair_in(heap, r);
    insert_with_incref(heap, arr, 0, v);
    assert_counted_once_then_freed(heap, r, "insert");
}

#[test]
fn an_overwrite_with_a_value_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    push_with_incref(heap, arr, Value::int(0));
    let v = pair_in(heap, r);
    set_at_with_rebind(heap, arr, 0, v);
    assert_counted_once_then_freed(heap, r, "overwrite");
}

#[test]
fn a_struct_put_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let st = struct_in(heap, r);
    let v = pair_in(heap, r);
    struct_put_with_rebind(heap, st, TableKey::keyword("k"), v);
    assert_counted_once_then_freed(heap, r, "struct put");
}

#[test]
fn a_struct_rebind_from_another_region_to_the_containers_own_is_not_counted() {
    // The displaced value lives elsewhere, so its reference is real and goes
    // away. The new one lives in the container's region and takes none.
    let (heap, r) = one_region();
    let st = struct_in(heap, r);
    let (elsewhere, other) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    struct_put_with_rebind(heap, st, TableKey::keyword("k"), elsewhere);
    assert_eq!(
        heap.region_rc(other),
        2,
        "the first store is a counted reference"
    );

    let v = pair_in(heap, r);
    struct_put_with_rebind(heap, st, TableKey::keyword("k"), v);
    assert_eq!(
        heap.region_rc(other),
        1,
        "the displaced value's reference is released"
    );
    assert_counted_once_then_freed(heap, r, "struct rebind");
}

#[test]
fn a_set_add_from_the_containers_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let set = set_in(heap, r);
    let v = pair_in(heap, r);
    set_add_with_incref(heap, set, v);
    assert_counted_once_then_freed(heap, r, "set add");
}

#[test]
fn a_box_store_from_the_boxs_own_region_is_not_counted() {
    let (heap, r) = one_region();
    let bx = box_in(heap, r);
    let v = pair_in(heap, r);
    lbox_store_with_rebind(heap, bx, v);
    assert_counted_once_then_freed(heap, r, "box store");
}

#[test]
fn a_pop_from_the_containers_own_region_hands_the_caller_one_reference() {
    // The popped value is the call's result, so the caller owns one reference to
    // its region, and the container gives back nothing it never took.
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    let v = pair_in(heap, r);
    push_with_incref(heap, arr, v);
    let popped = pop_with_decref(heap, arr);
    assert_eq!(region_of(heap, popped), Some(r));
    assert_eq!(
        heap.region_rc(r),
        2,
        "the caller's reference beside the test's own"
    );
    heap.decref_region(r);
    assert_counted_once_then_freed(heap, r, "pop");
}

#[test]
fn a_removal_from_the_containers_own_region_lowers_nothing() {
    let (heap, r) = one_region();
    let arr = array_in(heap, r);
    let a = pair_in(heap, r);
    let b = pair_in(heap, r);
    let c = pair_in(heap, r);
    extend_with_incref(heap, arr, &[a, b, c]);
    remove_at_with_decref(heap, arr, 0);
    drain_tail_with_decref(heap, arr, 1);
    set_at_with_rebind(heap, arr, 0, Value::int(0));
    assert_counted_once_then_freed(heap, r, "array removals");

    let (heap, r) = one_region();
    let st = struct_in(heap, r);
    let v = pair_in(heap, r);
    struct_put_with_rebind(heap, st, TableKey::keyword("k"), v);
    struct_remove_with_decref(heap, st, &TableKey::keyword("k"));
    assert_counted_once_then_freed(heap, r, "struct remove");

    let (heap, r) = one_region();
    let set = set_in(heap, r);
    let v = pair_in(heap, r);
    set_add_with_incref(heap, set, v);
    set_del_with_decref(heap, set, &v);
    assert_counted_once_then_freed(heap, r, "set delete");

    let (heap, r) = one_region();
    let bx = box_in(heap, r);
    let v = pair_in(heap, r);
    lbox_store_with_rebind(heap, bx, v);
    lbox_store_with_rebind(heap, bx, Value::NIL);
    assert_counted_once_then_freed(heap, r, "box overwrite");
}
