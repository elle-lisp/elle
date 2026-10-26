// audited: 2026-10-23
// The store of an @set: its members in its container's region, grown by the
// add funnel, owning nothing outside it.
//
// docs/impl/mutable.md
//
// A store on the Rust heap passes every membership, order and borrow pin, so
// those cannot tell where the members live. The placement, drop and
// foreign-shell pins can: each fails while the store is a `BTreeSet`, and holds
// once its members lie in the container's region. The model, free, shell and
// borrow pins hold on a sorted extent and on a tree of nodes in one extent, and
// guard what a move between the two must keep.

use std::collections::BTreeSet;

use elle::primitives::ctx::TestHeap;
use elle::syntax::Syntax;
use elle::value::arena::{region_of, set_add_with_incref, set_del_with_decref};
use elle::value::heap::{HeapObject, Pair};
use elle::value::repr::TAG_ARRAY;
use elle::value::store::SetStore;
use elle::Value;

fn ints(ns: &[i64]) -> BTreeSet<Value> {
    ns.iter().map(|&n| Value::int(n)).collect()
}

/// A heap value at `ptr`, for the region store's ownership check, which reads
/// nothing but the address.
fn address(ptr: *const Value) -> Value {
    Value::from_heap_ptr(ptr as *const (), TAG_ARRAY)
}

fn members(set: Value) -> Vec<Value> {
    set.set_mut_read()
        .expect("an @set")
        .iter()
        .copied()
        .collect()
}

// A `BTreeSet`'s node is a `malloc` the region store does not own. So the
// ownership check fails first, and the page-header read beside it never runs
// against a pointer with no header.
#[test]
fn a_grown_set_store_lies_in_its_containers_region() {
    let h = TestHeap::new();
    let set = h.ctx().set_mut(ints(&[1, 2, 3]));
    for n in 0..1000 {
        set_add_with_incref(h.heap(), set, Value::int(n));
    }
    let read = set.set_mut_read().expect("an @set");
    let first: *const Value = read.iter().next().expect("a member");
    assert!(
        h.heap().value_in_region_store(address(first)),
        "a grown @set keeps its members outside every region of its heap"
    );
    assert_eq!(
        h.heap().region_of_ptr(first as *const ()),
        region_of(h.heap(), set).expect("a heap value").get(),
        "a grown @set keeps its members in a region other than its own"
    );
}

#[test]
fn a_set_store_owns_nothing_to_drop() {
    assert!(
        !std::mem::needs_drop::<SetStore>(),
        "a set store owns memory outside its region's pages"
    );
}

#[test]
fn a_set_store_fits_the_shell_a_syntax_node_sets() {
    assert!(
        std::mem::size_of::<SetStore>() <= std::mem::size_of::<Syntax>(),
        "an @set's store grows every heap object past the size a syntax node sets"
    );
}

// A cross-thread channel delivers a pointer into the sender's heap, so a
// receiver can hold a store its own heap does not. A store built with eleven
// members has room for eleven: a sorted extent of that length, or one full
// node. A twelfth must grow it, and growing would claim an extent in a region
// the receiver cannot name.
#[test]
#[should_panic(expected = "no region of this heap")]
fn a_growing_add_on_another_heaps_set_panics() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let set = theirs
        .ctx()
        .set_mut(ints(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]));
    set_add_with_incref(ours.heap(), set, Value::int(12));
}

// The counterpart of the panic above: a delete claims nothing, an add after it
// fits the capacity the delete left, and an add of a member already present
// writes nothing. None of them needs a region, so each stays legal on a
// foreign store. The members are immediates, so no write here resolves a
// region either.
#[test]
fn a_write_within_capacity_on_another_heaps_set_edits_it() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let set = theirs.ctx().set_mut(ints(&[1, 2, 3]));
    assert!(set_del_with_decref(ours.heap(), set, &Value::int(2)));
    assert!(!set_del_with_decref(ours.heap(), set, &Value::int(9)));
    assert!(set_add_with_incref(ours.heap(), set, Value::int(0)));
    assert!(!set_add_with_incref(ours.heap(), set, Value::int(3)));
    assert_eq!(
        members(set),
        ints(&[0, 1, 3]).into_iter().collect::<Vec<_>>()
    );
}

// An add that splits a node takes the nodes it needs from those its deletes
// freed, so it claims nothing either. Twelve adds split the one full node into
// two leaves under a root, and two deletes merge them back, freeing two nodes.
// The two adds after them split that node again on the receiver's heap. A
// sorted extent holds them within the capacity its doublings left.
#[test]
fn an_add_into_freed_room_on_another_heaps_set_claims_nothing() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let set = theirs.ctx().set_mut(BTreeSet::new());
    for n in 0..12 {
        set_add_with_incref(theirs.heap(), set, Value::int(n));
    }
    for n in [11, 10] {
        assert!(set_del_with_decref(theirs.heap(), set, &Value::int(n)));
    }
    for n in [10, 11] {
        assert!(set_add_with_incref(ours.heap(), set, Value::int(n)));
    }
    let want: Vec<i64> = (0..12).collect();
    assert_eq!(members(set), ints(&want).into_iter().collect::<Vec<_>>());
}

// An @set with no destructor leaves the destructor list, and the free finds it
// on the reference list instead. Missing from both, a member the set holds in
// another region would keep that region alive after the set's region frees.
#[test]
fn freeing_a_sets_region_releases_its_members_region() {
    let h = TestHeap::new();
    let other = h.heap().new_runtime_region();
    let held = h.heap().alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
        other,
    );
    let set = h.ctx().set_mut(BTreeSet::from([held]));
    assert_eq!(
        h.heap().region_rc(other),
        2,
        "an @set holds a reference to its member's region"
    );
    let own = region_of(h.heap(), set).expect("an @set lies in a region");
    h.heap().decref_region(own);
    assert_eq!(
        h.heap().region_rc(other),
        1,
        "freeing an @set's region keeps the reference to its member's region"
    );
}

// Forty members fill more than one node, so the free must read every node of
// the store. A free that reads only the root's members keeps the regions of
// the rest alive.
#[test]
fn freeing_a_sets_region_releases_every_members_region() {
    let h = TestHeap::new();
    let set = h.ctx().set_mut(BTreeSet::new());
    let mut others = Vec::new();
    for n in 0..40 {
        let other = h.heap().new_runtime_region();
        let held = h.heap().alloc_in_region(
            HeapObject::Pair(Pair::new(Value::int(n), Value::NIL)),
            other,
        );
        set_add_with_incref(h.heap(), set, held);
        others.push(other);
    }
    let own = region_of(h.heap(), set).expect("an @set lies in a region");
    h.heap().decref_region(own);
    for (n, &other) in others.iter().enumerate() {
        assert_eq!(
            h.heap().region_rc(other),
            1,
            "freeing an @set's region keeps the reference to member {n}'s region"
        );
    }
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_del_under_a_set_read_panics() {
    let h = TestHeap::new();
    let set = h.ctx().set_mut(ints(&[1]));
    let _read = set.set_mut_read().unwrap();
    set_del_with_decref(h.heap(), set, &Value::int(1));
}
