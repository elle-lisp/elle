// audited: 2026-10-30
//! A store whose new value lies in the displaced value's region moves no count, and frees nothing.
//!
//! docs/impl/region/rules.md

// Each test stores a value born in region `r` into a container in another
// region. It then gives up its own reference, so the container holds `r`'s last
// count. Next it stores into the same slot either the same value or a sibling
// born in `r`. The counter-factual: a funnel that lowers the displaced value's
// count before it raises the new one's frees `r` there, under the value it is
// storing.

use super::*;
use crate::value::heap::{CellOrigin, TableKey};

/// A fresh heap, and a pair in a region of its own whose one count is the test's.
fn heap_and_value() -> (&'static mut FiberHeap, Value, RuntimeRegion) {
    let heap = unsafe { &mut *leaked_test_heap() };
    let (v, r) = alloc_in_fresh_region(heap, pair());
    (heap, v, r)
}

fn pair() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::int(1), Value::NIL))
}

/// Store `v`, then release the test's reference, then store `again` into the
/// same slot. Region `r` must stay live on the container's one count, and the
/// store of nil that displaces `again` must free it.
fn assert_restore_keeps_region(
    heap: &mut FiberHeap,
    r: RuntimeRegion,
    v: Value,
    again: Value,
    store: impl Fn(&mut FiberHeap, Value),
    what: &str,
) {
    store(heap, v);
    assert_eq!(heap.region_rc(r), 2, "{what}: the first store is counted");
    heap.decref_region(r);
    assert_eq!(heap.region_rc(r), 1, "{what}: the container holds r alone");

    store(heap, again);
    assert_eq!(
        heap.region_rc(r),
        1,
        "{what}: a store from the displaced value's region moved a count",
    );
    assert_eq!(
        region_of(heap, again),
        Some(r),
        "{what}: the stored value's region was freed under it",
    );

    store(heap, Value::NIL);
    assert_eq!(heap.region_rc(r), 0, "{what}: displacing it frees r");
}

fn capture_cell_elsewhere(heap: &mut FiberHeap) -> Value {
    let c = heap.new_runtime_region();
    crate::value::build::capture_cell(heap, Value::NIL, CellOrigin::Runtime, c)
}

#[test]
fn a_capture_cell_store_of_the_value_it_holds_keeps_its_region() {
    let (heap, v, r) = heap_and_value();
    let cell = capture_cell_elsewhere(heap);
    let store = move |h: &mut FiberHeap, x| {
        capture_store_with_rebind(h, cell, x);
    };
    assert_restore_keeps_region(heap, r, v, v, store, "capture cell, same value");
}

#[test]
fn a_capture_cell_store_of_a_sibling_keeps_its_region() {
    let (heap, v, r) = heap_and_value();
    let sibling = heap.alloc_in_region(pair(), r);
    let cell = capture_cell_elsewhere(heap);
    let store = move |h: &mut FiberHeap, x| {
        capture_store_with_rebind(h, cell, x);
    };
    assert_restore_keeps_region(heap, r, v, sibling, store, "capture cell, sibling");
}

#[test]
fn a_box_store_of_the_value_it_holds_keeps_its_region() {
    let (heap, v, r) = heap_and_value();
    let c = heap.new_runtime_region();
    let bx = crate::value::build::lbox(heap, Value::NIL, c);
    let store = move |h: &mut FiberHeap, x| {
        lbox_store_with_rebind(h, bx, x);
    };
    assert_restore_keeps_region(heap, r, v, v, store, "box");
}

#[test]
fn an_array_slot_store_of_the_value_it_holds_keeps_its_region() {
    let (heap, v, r) = heap_and_value();
    let (arr, _) = array_in_fresh_region(heap);
    push_with_incref(heap, arr, Value::NIL);
    let store = move |h: &mut FiberHeap, x| {
        set_at_with_rebind(h, arr, 0, x);
    };
    assert_restore_keeps_region(heap, r, v, v, store, "@array slot");
}

#[test]
fn a_struct_entry_store_of_the_value_it_holds_keeps_its_region() {
    let (heap, v, r) = heap_and_value();
    let (st, _) = struct_in_fresh_region(heap);
    struct_put_with_rebind(heap, st, TableKey::keyword("k"), Value::NIL);
    let store = move |h: &mut FiberHeap, x| {
        struct_put_with_rebind(h, st, TableKey::keyword("k"), x);
    };
    assert_restore_keeps_region(heap, r, v, v, store, "@struct entry");
}
