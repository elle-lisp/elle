// audited: 2026-09-29
// A join takes one reference on a live counted region, and refuses the rest.
//
// docs/impl/region/colocation.md
//
// A refused join costs nothing: the site mints fresh, which is always legal. So
// every refusal below is pinned against the wrong answer a join that never
// refuses would give.

use super::*;

/// A live `Counted` region.
fn live_region(store: &mut RegionStore) -> RuntimeRegion {
    let r = store.new_runtime_region();
    store.alloc_obj(r, cons_obj());
    r
}

#[test]
fn a_join_takes_one_reference_on_a_live_counted_region() {
    let mut store = RegionStore::default();
    let r = live_region(&mut store);

    assert_eq!(
        store.join(r),
        Some(r),
        "a live counted region admits the join"
    );
    assert_eq!(store.rc(r), 2, "the join holds a reference of its own");

    // The joining site releases its reference where a fresh mint's would go.
    // The region's own holder is still there, so its objects stay.
    store.decref(r);
    assert_eq!(
        store.region_obj_count(r),
        1,
        "the join's release frees nothing"
    );
    store.decref(r);
    assert_eq!(
        store.region_obj_count(r),
        0,
        "the last release frees the region"
    );
}

#[test]
fn a_join_refuses_an_owned_region() {
    // An `Owned` region has no count, so a reference taken on it holds nothing:
    // the owner's subtree drop frees it whatever the joining site still holds.
    let mut store = RegionStore::default();
    let parent = live_region(&mut store);
    let child = live_region(&mut store);
    store.adopt_region(parent, child);

    assert_eq!(
        store.join(child),
        None,
        "a join into an Owned region holds nothing"
    );
}

#[test]
fn a_join_refuses_an_id_that_names_no_live_region() {
    let mut store = RegionStore::default();
    let reserved = store.new_runtime_region();
    assert_eq!(
        store.join(reserved),
        None,
        "a minted id nothing allocated into is not a region to join",
    );

    let freed = live_region(&mut store);
    store.decref(freed);
    assert_eq!(
        store.join(freed),
        None,
        "a freed region is not a region to join"
    );
}

#[test]
fn a_joined_region_is_never_adopted() {
    // Adoption consumes the whole count and hands the region to one owner. A
    // joined region's count belongs to every site that joined it, so an adopt
    // would free it under their references.
    let mut store = RegionStore::default();
    let parent = live_region(&mut store);
    let joined = live_region(&mut store);
    assert_eq!(store.join(joined), Some(joined));

    store.adopt_region(parent, joined);

    assert!(
        !store.region_is_owned(joined),
        "the adopt left a joined region Counted",
    );
    assert_eq!(store.rc(joined), 2, "the adopt consumed no reference");
}
