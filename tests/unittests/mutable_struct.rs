// audited: 2026-10-23
// The store of a @struct: its entries in its container's region, grown by the
// put funnel, owning nothing outside it.
//
// docs/impl/mutable.md
//
// A `BTreeMap` passes every lookup, order, rebind and free pin, so those cannot
// tell where the entries live. The placement, drop, census and foreign-shell
// pins can: each fails while the store is a `BTreeMap`, and holds once its
// entries lie in the container's region.

use elle::hir::region::RuntimeRegion;
use elle::primitives::ctx::TestHeap;
use elle::value::arena::{region_of, struct_put_with_rebind, struct_remove_with_decref};
use elle::value::heap::{HeapObject, HeapTag, Pair};
use elle::value::repr::TAG_ARRAY;
use elle::value::store::StructStore;
use elle::value::types::TableKey;
use elle::Value;

/// A @struct on `h` holding each key of `keys` under its own value.
fn ints(h: &TestHeap, keys: std::ops::Range<i64>) -> Value {
    let st = h.ctx().struct_mut();
    for k in keys {
        struct_put_with_rebind(h.heap(), st, TableKey::Int(k), Value::int(k));
    }
    st
}

/// A pair in a fresh region of `h`, and that region.
fn pair_elsewhere(h: &TestHeap, n: i64) -> (Value, RuntimeRegion) {
    let other = h.heap().new_runtime_region();
    let pair = HeapObject::Pair(Pair::new(Value::int(n), Value::NIL));
    (h.heap().alloc_in_region(pair, other), other)
}

/// A string spelled `text` in `region` of `h`.
fn string_in(h: &TestHeap, text: &str, region: RuntimeRegion) -> Value {
    let s = h
        .heap()
        .alloc_region_slice_in_region(text.as_bytes(), region);
    h.heap().alloc_in_region(
        HeapObject::LString {
            s,
            traits: Value::NIL,
        },
        region,
    )
}

fn entries(st: Value) -> Vec<(TableKey, Value)> {
    let read = st.struct_mut_read().expect("an @struct");
    read.iter().map(|(k, v)| (*k, *v)).collect()
}

// A `BTreeMap`'s node is a `malloc` the region store does not own. So the
// ownership check fails first, and the page-header read beside it never runs
// against a pointer with no header.
#[test]
fn a_grown_struct_store_lies_in_its_containers_region() {
    let h = TestHeap::new();
    let st = ints(&h, 0..1000);
    let read = st.struct_mut_read().expect("an @struct");
    let (_, first) = read.iter().next().expect("an entry");
    let at = Value::from_heap_ptr(first as *const Value as *const (), TAG_ARRAY);
    assert!(
        h.heap().value_in_region_store(at),
        "a grown @struct keeps its entries outside every region of its heap"
    );
    assert_eq!(
        h.heap()
            .region_of_ptr(at.as_heap_ptr().expect("an address")),
        region_of(h.heap(), st).expect("a heap value").get(),
        "a grown @struct keeps its entries in a region other than its own"
    );
}

#[test]
fn a_struct_store_owns_nothing_to_drop() {
    assert!(
        !std::mem::needs_drop::<StructStore>(),
        "a struct store owns memory outside its region's pages"
    );
}

// The census counts the node extent, because that is what a dump copies. One
// node has room for eleven entries, so a census that counts the entries alone
// charges a one-entry store a tenth of its bytes.
#[test]
fn a_struct_census_counts_its_node_extent() {
    let h = TestHeap::new();
    ints(&h, 0..1);
    let census = h.heap().census();
    let row = census
        .tags
        .iter()
        .find(|row| row.tag == HeapTag::LStructMut)
        .expect("a census row for the @struct");
    let shell = std::mem::size_of::<HeapObject>();
    let node = 11 * std::mem::size_of::<(TableKey, Value)>();
    assert!(
        row.bytes >= row.count * shell + node,
        "the census charges a @struct of one entry {} bytes",
        row.bytes
    );
}

// A cross-thread channel delivers a pointer into the sender's heap. A store of
// eleven entries fills its one node, so a twelfth must grow it, into a region
// the receiver cannot name.
#[test]
#[should_panic(expected = "no region of this heap")]
fn a_growing_put_on_another_heaps_struct_panics() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let st = ints(&theirs, 0..11);
    struct_put_with_rebind(ours.heap(), st, TableKey::Int(11), Value::int(11));
}

// The counterpart: a delete claims nothing, a put after it fits the room the
// delete left, and a rebind replaces a value in place. The keys and values are
// immediates, so no write here resolves a region either.
#[test]
fn a_write_within_capacity_on_another_heaps_struct_edits_it() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let st = ints(&theirs, 1..4);
    let gone = struct_remove_with_decref(ours.heap(), st, &TableKey::Int(2));
    assert_eq!(gone, Some(Value::int(2)));
    assert_eq!(
        struct_remove_with_decref(ours.heap(), st, &TableKey::Int(9)),
        None
    );
    assert_eq!(
        struct_put_with_rebind(ours.heap(), st, TableKey::Int(0), Value::int(0)),
        None
    );
    let old = struct_put_with_rebind(ours.heap(), st, TableKey::Int(3), Value::int(7));
    assert_eq!(old, Some(Value::int(3)));
    let want = [(0, 0), (1, 1), (3, 7)].map(|(k, v)| (TableKey::Int(k), Value::int(v)));
    assert_eq!(entries(st), want);
}

// Twelve puts split the one full node into two leaves under a root, and two
// deletes merge them back, freeing two nodes. The two puts after them split
// that node again on the receiver's heap, from the nodes the merge freed.
#[test]
fn a_put_into_freed_room_on_another_heaps_struct_claims_nothing() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let st = ints(&theirs, 0..12);
    for k in [11, 10] {
        assert!(struct_remove_with_decref(theirs.heap(), st, &TableKey::Int(k)).is_some());
    }
    for k in [10, 11] {
        struct_put_with_rebind(ours.heap(), st, TableKey::Int(k), Value::int(k));
    }
    let want: Vec<_> = (0..12).map(|k| (TableKey::Int(k), Value::int(k))).collect();
    assert_eq!(entries(st), want);
}

// A put interns a new string key into the struct's own region. A rebind under
// an equal key spelled in a third region keeps the key the store holds, so the
// store never comes to point into the caller's region.
#[test]
fn a_rebind_keeps_the_stored_key_and_answers_the_displaced_value() {
    let h = TestHeap::new();
    let st = h.ctx().struct_mut();
    let own = region_of(h.heap(), st).expect("an @struct lies in a region");
    let key = |h: &TestHeap| {
        let region = h.heap().new_runtime_region();
        TableKey::from_value(&string_in(h, "k", region)).expect("a string key")
    };
    assert_eq!(
        struct_put_with_rebind(h.heap(), st, key(&h), Value::int(1)),
        None
    );
    let stored = entries(st)[0].0.to_value();
    assert_eq!(
        region_of(h.heap(), stored),
        Some(own),
        "a put interns its key"
    );
    let old = struct_put_with_rebind(h.heap(), st, key(&h), Value::int(2));
    assert_eq!(old, Some(Value::int(1)));
    let after = entries(st);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].0.to_value().as_heap_ptr(), stored.as_heap_ptr());
    assert_eq!(after[0].1, Value::int(2));
}

// A @struct with no destructor leaves the destructor list, and the free finds
// it on the reference list instead. Forty entries fill more than one node, so
// the free must read every node. A free that misses an entry keeps the region
// its value or its key lies in alive after the struct's region frees.
#[test]
fn freeing_a_structs_region_releases_every_values_and_keys_region() {
    let h = TestHeap::new();
    let st = h.ctx().struct_mut();
    let mut held = Vec::new();
    for n in 0..40 {
        let (k, key_region) = pair_elsewhere(&h, n);
        let (v, value_region) = pair_elsewhere(&h, n);
        let key = TableKey::from_value(&k).expect("a pair key");
        struct_put_with_rebind(h.heap(), st, key, v);
        held.push((n, key_region, value_region));
    }
    for &(n, key_region, value_region) in &held {
        assert_eq!(
            h.heap().region_rc(key_region),
            2,
            "the struct holds key {n}"
        );
        assert_eq!(
            h.heap().region_rc(value_region),
            2,
            "the struct holds value {n}"
        );
    }
    let own = region_of(h.heap(), st).expect("an @struct lies in a region");
    h.heap().decref_region(own);
    for (n, key_region, value_region) in held {
        assert_eq!(
            h.heap().region_rc(key_region),
            1,
            "the free keeps key {n}'s region"
        );
        assert_eq!(
            h.heap().region_rc(value_region),
            1,
            "the free keeps value {n}'s region"
        );
    }
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_remove_under_a_struct_read_panics() {
    let h = TestHeap::new();
    let st = ints(&h, 0..1);
    let _read = st.struct_mut_read().unwrap();
    struct_remove_with_decref(h.heap(), st, &TableKey::Int(0));
}
