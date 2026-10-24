// audited: 2026-10-22
// The store of a box and a capture cell: one Value inline in its shell, owning
// nothing outside it.
//
// docs/impl/mutable.md
//
// A store on the Rust heap passes every read and write pin, so those cannot
// tell where the value lives. The drop and census pins can: each fails while
// the store is an `Rc<RefCell<Value>>`, and holds once the value lies inside
// the shell. The cascade, foreign-shell and write pins hold on both, and guard
// what the move must keep.

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{capture_store_with_rebind, lbox_store_with_rebind, region_of};
use elle::value::heap::{CellOrigin, HeapObject, HeapTag, Pair};
use elle::value::store::CellStore;
use elle::Value;

#[test]
fn a_cell_store_owns_nothing_to_drop() {
    assert!(
        !std::mem::needs_drop::<CellStore>(),
        "a cell store owns memory outside its shell"
    );
}

// The census counts each object's shell, and its payload wherever the payload
// lies. A value inside the shell is part of the shell, so counting it again
// would charge a cell for bytes no page holds.
#[test]
fn a_cell_census_counts_its_shell_alone() {
    let h = TestHeap::new();
    h.ctx().lbox(Value::int(1));
    h.ctx().capture_cell(Value::int(2), CellOrigin::Runtime);
    let census = h.heap().census();
    for tag in [HeapTag::LBox, HeapTag::CaptureCell] {
        let row = census
            .tags
            .iter()
            .find(|row| row.tag == tag)
            .expect("a census row for each variant built");
        assert_eq!(
            row.bytes,
            row.count * std::mem::size_of::<HeapObject>(),
            "the census charges a {tag:?} for bytes outside its shell"
        );
    }
}

// A cell with no destructor leaves the destructor list, and the free finds it
// on the reference list instead. Missing from both, a value the cell holds in
// another region would keep that region alive after the cell's region frees.
#[test]
fn freeing_a_cells_region_releases_its_values_region() {
    let h = TestHeap::new();
    let other = h.heap().new_runtime_region();
    let held = h.heap().alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
        other,
    );
    let cell = h.ctx().capture_cell(held, CellOrigin::Runtime);
    assert_eq!(
        h.heap().region_rc(other),
        2,
        "a cell holds a reference to its value's region"
    );
    let own = region_of(h.heap(), cell).expect("a cell lies in a region");
    h.heap().decref_region(own);
    assert_eq!(
        h.heap().region_rc(other),
        1,
        "freeing a cell's region keeps the reference to its value's region"
    );
}

// A cross-thread channel delivers a pointer into the sender's heap. A store
// into a cell claims nothing, so it needs no region of the funnel's heap, and
// it stays legal on a cell another heap holds.
#[test]
fn a_store_into_another_heaps_cell_edits_it() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let bx = theirs.ctx().lbox(Value::int(1));
    let cell = theirs
        .ctx()
        .capture_cell(Value::int(1), CellOrigin::Runtime);
    assert_eq!(
        lbox_store_with_rebind(ours.heap(), bx, Value::int(2)),
        Value::int(1)
    );
    assert_eq!(
        capture_store_with_rebind(ours.heap(), cell, Value::int(3)),
        Value::int(1)
    );
    assert_eq!(bx.lbox_get(), Some(Value::int(2)));
    assert_eq!(cell.capture_cell_get(), Some(Value::int(3)));
}

// A box keeps the value of its last store, and a store answers the value it
// displaced, through any number of writes.
#[test]
fn a_box_answers_each_store_with_the_value_it_displaced() {
    let h = TestHeap::new();
    let bx = h.ctx().lbox(Value::int(0));
    for n in 1..100 {
        assert_eq!(
            lbox_store_with_rebind(h.heap(), bx, Value::int(n)),
            Value::int(n - 1)
        );
    }
    assert_eq!(bx.lbox_get(), Some(Value::int(99)));
}
