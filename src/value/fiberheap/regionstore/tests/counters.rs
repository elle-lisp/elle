// audited: 2026-09-29
// Each reclamation counter moves at the store event it names, and the forest counters close.
//
// docs/impl/region/diagnostics.md
//
// Every assertion here reads a count that must move. A store that counted
// nothing would still reclaim correctly, so no other test in this module can
// see a counter that is stuck at zero or bumped on the wrong path.

use super::*;

/// A mutable array in `region` that holds `vals`, the way a container stores
/// what it was given: the allocation records an edge to each value's region.
fn holder(store: &mut RegionStore, region: RuntimeRegion, vals: Vec<Value>) -> Value {
    store.alloc_obj(
        region,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vals)),
            traits: Value::NIL,
        },
    )
}

/// The identity diagnostics.md states: every adoption has ended in an owned
/// free, a rescue or an extraction, or its region is still owned.
fn assert_forest_closes(store: &RegionStore, when: &str) {
    let c = store.reclaim_counters();
    assert_eq!(
        c.adopts,
        c.owned_frees + c.rescues + c.extracts + store.owned_count(),
        "{when}: adopts must equal owned-frees + rescues + extracts + owned, got {c:?} \
         with {} owned",
        store.owned_count()
    );
}

#[test]
fn a_count_reaching_zero_counts_the_region_its_page_and_its_objects() {
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    store.alloc_obj(r, cons_obj());
    store.alloc_obj(r, cons_obj());

    store.decref(r);
    let c = store.reclaim_counters();
    assert_eq!(c.region_frees, 1, "one region freed");
    assert_eq!(c.page_frees, 1, "two small objects share one page");
    assert_eq!(c.object_frees, 2, "both objects freed with it");
    assert_eq!(c.owned_frees, 0, "a counted region is not an owned free");
}

#[test]
fn an_adoption_counts_once_and_the_owners_drop_frees_the_member_as_owned() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    let child_val = store.alloc_obj(child, cons_obj());
    holder(&mut store, parent, vec![child_val]);

    store.adopt_region(parent, child);
    let c = store.reclaim_counters();
    assert_eq!(c.adopts, 1, "one adoption");
    assert_eq!(c.adopts_into_empty, 0, "the owner held an object");
    assert_eq!(store.owned_count(), 1, "the child is owned");
    assert_forest_closes(&store, "after the adopt");

    store.decref(parent);
    let c = store.reclaim_counters();
    assert_eq!(c.owned_frees, 1, "the owner's drop freed its member");
    assert_eq!(c.owned_free_pages, 1, "the member held one page");
    assert_eq!(c.owned_free_objects, 1, "the member held one object");
    assert_eq!(c.owned_one_page_frees, 1, "the member held one page");
    assert_eq!(c.region_frees, 2, "the owner and its member both freed");
    assert_eq!(c.object_frees, 2, "one object each");
    assert_eq!(store.owned_count(), 0, "nothing is owned after the drop");
    assert_forest_closes(&store, "after the drop");
}

#[test]
fn an_adoption_into_an_owner_that_holds_nothing_counts_as_into_empty() {
    // An owner node never allocates; a region nothing allocated into yet is the
    // same case to the store. Either way the owner has no page to share.
    let mut store = RegionStore::default();
    let node = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(child, cons_obj());

    store.adopt_region(node, child);
    let c = store.reclaim_counters();
    assert_eq!(c.adopts, 1);
    assert_eq!(c.adopts_into_empty, 1, "the owner held no object");
}

#[test]
fn a_rescue_counts_the_rescued_region_and_the_subtree_it_keeps() {
    // The rescued member keeps its owned grandchild, so two regions survive the
    // root's drop and one of them is still owned.
    let mut store = RegionStore::default();
    let root = store.new_runtime_region();
    let member = store.new_runtime_region();
    let grand = store.new_runtime_region();
    let cell = store.new_runtime_region();
    let member_val = store.alloc_obj(member, cons_obj());
    let grand_val = store.alloc_obj(grand, cons_obj());
    holder(&mut store, member, vec![grand_val]);
    holder(&mut store, root, vec![member_val]);
    holder(&mut store, cell, vec![member_val]);
    store.adopt_region(root, member);
    store.adopt_region(member, grand);

    store.decref(root);
    let c = store.reclaim_counters();
    assert_eq!(c.rescues, 1, "the member was rescued");
    assert_eq!(
        c.rescue_survivors, 2,
        "the member and the grandchild it keeps"
    );
    assert_eq!(c.owned_frees, 0, "the drop freed no owned region");
    assert_eq!(c.region_frees, 1, "the root alone freed");
    assert_eq!(store.owned_count(), 1, "the grandchild is still owned");
    assert_forest_closes(&store, "after the rescue");

    store.decref(cell);
    let c = store.reclaim_counters();
    assert_eq!(
        c.owned_frees, 1,
        "the rescued member's drop frees the grandchild as owned"
    );
    assert_forest_closes(&store, "after the rescued member's drop");
}

#[test]
fn an_extraction_counts_and_the_region_leaves_the_forest() {
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    let child_val = store.alloc_obj(child, cons_obj());
    holder(&mut store, parent, vec![child_val]);
    store.adopt_region(parent, child);

    store.extract_owned_region(child);
    let c = store.reclaim_counters();
    assert_eq!(c.extracts, 1, "one owned region extracted");
    assert_eq!(
        store.owned_count(),
        0,
        "the extracted region is counted again"
    );
    assert_forest_closes(&store, "after the extraction");

    // A counted region is not owned, so extracting it again changes nothing.
    store.extract_owned_region(child);
    assert_eq!(
        store.reclaim_counters().extracts,
        1,
        "a counted region is no extraction"
    );
}

#[test]
fn a_reparent_counts_each_member_handed_over() {
    let mut store = RegionStore::default();
    let from = store.new_runtime_region();
    let to = store.new_runtime_region();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    store.alloc_obj(from, cons_obj());
    store.alloc_obj(to, cons_obj());
    store.alloc_obj(a, cons_obj());
    store.alloc_obj(b, cons_obj());
    store.adopt_region(from, a);
    store.adopt_region(from, b);

    store.reparent_owned_children(from, to);
    let c = store.reclaim_counters();
    assert_eq!(c.reparents, 2, "both members handed over");
    assert_eq!(store.owned_count(), 2, "a reparented region stays owned");
    assert_forest_closes(&store, "after the reparent");
}

#[test]
fn a_group_free_counts_every_member() {
    let mut store = RegionStore::default();
    let a = store.new_runtime_region();
    let b = store.new_runtime_region();
    store.alloc_obj(a, cons_obj());
    store.alloc_obj(b, cons_obj());

    store.free_region_group(&[a, b]);
    let c = store.reclaim_counters();
    assert_eq!(c.region_frees, 2, "both members of the group freed");
    assert_eq!(c.object_frees, 2);
}

#[test]
fn a_teardown_counts_nothing() {
    // The teardown frees what the program left; the program's code caused none
    // of it, so none of it is a reclamation.
    let mut store = RegionStore::default();
    let parent = store.new_runtime_region();
    let child = store.new_runtime_region();
    store.alloc_obj(parent, cons_obj());
    store.alloc_obj(child, cons_obj());
    store.adopt_region(parent, child);
    let before = store.reclaim_counters();
    assert_eq!(before.adopts, 1, "precondition: the adoption counted");

    store.teardown_all();
    assert_eq!(
        store.reclaim_counters(),
        before,
        "a teardown moves no counter"
    );
}
