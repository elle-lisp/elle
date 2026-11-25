// audited: 2026-11-25
//! A region lives and dies without allocating on the Rust heap once the store has freed one of the same shape.
//!
//! docs/impl/region/ownership.md

use super::*;

// Each shape is built and freed once first, so the store's working sets and its
// page cache have grown to what a free of that shape needs. The counter-factual
// is a free that builds its sets afresh each time: a lone region then costs
// several allocations (the root list, the member list, the dying set, the
// frontier), and a free with an edge costs more.

/// Build a region with `make` and free it, to warm the store. Then build it
/// again, and answer how many allocations its free made.
fn free_allocations(
    store: &mut RegionStore,
    make: impl Fn(&mut RegionStore) -> RuntimeRegion,
) -> u64 {
    let warm = make(store);
    store.decref(warm);
    let r = make(store);
    let (_, made) = crate::testalloc::allocations(|| store.decref(r));
    made
}

#[test]
fn freeing_a_lone_region_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = free_allocations(&mut store, |s| {
        let r = s.new_runtime_region();
        s.alloc_obj(r, cons_obj());
        r
    });
    assert_eq!(made, 0, "a region with no edge frees without allocating");
}

#[test]
fn freeing_a_region_with_an_edge_allocates_nothing() {
    let mut store = RegionStore::default();
    let target = store.new_runtime_region();
    let held = store.alloc_obj(target, cons_obj());
    let made = free_allocations(&mut store, |s| {
        let r = s.new_runtime_region();
        holder(s, r, &[held]);
        r
    });
    assert_eq!(
        made, 0,
        "the frontier walk and its release allocate nothing"
    );
    assert_eq!(store.rc(target), 1, "the target keeps its own reference");
}

#[test]
fn a_cascade_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = free_allocations(&mut store, |s| {
        let inner = s.new_runtime_region();
        let held = s.alloc_obj(inner, cons_obj());
        let r = s.new_runtime_region();
        holder(s, r, &[held]);
        // The holder's edge is now the inner region's only reference, so the
        // holder's free cascades into it.
        s.decref(inner);
        r
    });
    assert_eq!(made, 0, "a free that cascades allocates nothing");
}

#[test]
fn an_owned_subtree_drop_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = free_allocations(&mut store, |s| {
        let owner = s.new_runtime_region();
        s.alloc_obj(owner, cons_obj());
        let child = s.new_runtime_region();
        s.alloc_obj(child, cons_obj());
        s.adopt_region(owner, child);
        owner
    });
    assert_eq!(made, 0, "the owner's subtree drop allocates nothing");
}

#[test]
fn a_group_free_allocates_nothing() {
    let mut store = RegionStore::default();
    let pair = |s: &mut RegionStore| {
        let a = s.new_runtime_region();
        s.alloc_obj(a, cons_obj());
        let b = s.new_runtime_region();
        s.alloc_obj(b, cons_obj());
        [a, b]
    };
    let warm = pair(&mut store);
    store.free_region_group(&warm);
    let members = pair(&mut store);
    let (_, made) = crate::testalloc::allocations(|| store.free_region_group(&members));
    assert_eq!(made, 0, "a group free allocates nothing");
}

// Making a region allocates nothing either, once a region of the same shape has
// lived and died: the free keeps the dead region's entry with its lists' capacity,
// and the next region is built out of it. The counter-factual is an entry built
// afresh for each region, whose page list, object lists and edge tables each
// allocate on their first push.

/// Build a region with `make` and free it, to warm the store. Then build and free
/// it again, and answer how many allocations that second life made.
fn lifetime_allocations(
    store: &mut RegionStore,
    make: impl Fn(&mut RegionStore) -> RuntimeRegion,
) -> u64 {
    let warm = make(store);
    store.decref(warm);
    let ((), made) = crate::testalloc::allocations(|| {
        let r = make(store);
        store.decref(r);
    });
    made
}

#[test]
fn making_a_lone_region_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = lifetime_allocations(&mut store, |s| {
        let r = s.new_runtime_region();
        s.alloc_obj(r, cons_obj());
        r
    });
    assert_eq!(made, 0, "a lone region lives and dies without allocating");
}

#[test]
fn making_a_region_with_an_edge_allocates_nothing() {
    let mut store = RegionStore::default();
    let target = store.new_runtime_region();
    let held = store.alloc_obj(target, cons_obj());
    let made = lifetime_allocations(&mut store, |s| {
        let r = s.new_runtime_region();
        holder(s, r, &[held]);
        r
    });
    assert_eq!(
        made, 0,
        "recording the edge, in both tables, allocates nothing"
    );
}

#[test]
fn making_a_region_that_holds_a_fresh_one_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = lifetime_allocations(&mut store, |s| {
        let inner = s.new_runtime_region();
        let held = s.alloc_obj(inner, cons_obj());
        let r = s.new_runtime_region();
        holder(s, r, &[held]);
        s.decref(inner);
        r
    });
    assert_eq!(
        made, 0,
        "a fresh target's incoming table is a kept entry's too"
    );
}

#[test]
fn making_an_owned_subtree_allocates_nothing() {
    let mut store = RegionStore::default();
    let made = lifetime_allocations(&mut store, |s| {
        let owner = s.new_runtime_region();
        s.alloc_obj(owner, cons_obj());
        let child = s.new_runtime_region();
        s.alloc_obj(child, cons_obj());
        s.adopt_region(owner, child);
        owner
    });
    assert_eq!(made, 0, "the owner's child list is a kept entry's too");
}

#[test]
fn the_store_keeps_a_bounded_number_of_entries() {
    let mut store = RegionStore::default();
    let regions: Vec<RuntimeRegion> = (0..KEPT_ENTRIES + 10)
        .map(|_| {
            let r = store.new_runtime_region();
            store.alloc_obj(r, cons_obj());
            r
        })
        .collect();
    for r in regions {
        store.decref(r);
    }
    assert_eq!(
        store.kept_entries(),
        KEPT_ENTRIES,
        "a free past the bound drops the entry it would keep"
    );
}

#[test]
fn a_free_drops_an_entry_whose_lists_outgrew_the_bound() {
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    for _ in 0..=KEPT_ENTRY_ROOM {
        store.alloc_obj(r, cons_obj());
    }
    store.decref(r);
    assert_eq!(
        store.kept_entries(),
        0,
        "a large region's object list does not stay resident"
    );
}
