// audited: 2026-09-09
// One generation counter per physical region id, so a stale deref is named at
// the deref instead of resolving to a dead region.
//
// docs/impl/region/generations.md
//
// The counter is bumped on every free and stamped into each claimed page's
// header. The region-of funnel compares the two in a debug build, which is what
// turns a stale-but-intact read into a panic at the site that made it — and
// what makes the three tests below debug-only.

use super::*;

#[test]
fn generation_bumps_on_free_and_recycled_id_mints_at_bumped_generation() {
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    let v = store.alloc_obj(r, cons_obj());
    assert_eq!(store.generation_raw(r.get()), 0);
    assert_eq!(store.region_of_ptr(v.as_heap_ptr().unwrap()), r.get());

    store.decref(r); // rc 1→0: freed, id recycled
    assert_eq!(
        store.generation_raw(r.get()),
        1,
        "free must bump the id's generation"
    );

    let r2 = store.new_runtime_region();
    assert_eq!(r2, r, "freed id is recycled");
    let v2 = store.alloc_obj(r2, cons_obj());
    // The new incarnation's pages carry the bumped generation: a live value
    // resolves to its region without tripping the stale-deref check.
    assert_eq!(store.region_of_ptr(v2.as_heap_ptr().unwrap()), r2.get());
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stale region")]
fn stale_value_deref_panics_after_region_free() {
    // The counterfactual for the whole mechanism: without the generation
    // check, region_of on a freed-but-cached page silently returns the dead
    // region's id — the stale-but-intact read the plain VM cannot tell from
    // a live one. Debug builds must panic at the deref instead.
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    let v = store.alloc_obj(r, cons_obj());
    let ptr = v.as_heap_ptr().unwrap();
    store.decref(r); // region freed; its page sits stale-but-intact in the cache
    let _ = store.region_of_ptr(ptr);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stale region")]
fn stale_value_deref_panics_after_id_recycle() {
    // Recycling the id mints the next region at generation g+1; the stale
    // page still carries g. The check must catch the stale pointer even
    // though its region id is live again under a new incarnation.
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    let v = store.alloc_obj(r, cons_obj());
    let ptr = v.as_heap_ptr().unwrap();
    store.decref(r);
    let r2 = store.new_runtime_region(); // recycles the same physical id
    assert_eq!(r2, r, "freed id is recycled");
    let _ = store.region_of_ptr(ptr); // page gen 0 vs current gen 1
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stale region")]
fn stale_value_deref_panics_after_teardown_all() {
    // Wholesale teardown (fiber death) returns pages without the RC path;
    // it must bump generations all the same.
    let mut store = RegionStore::default();
    let v = store.alloc_obj(rr(4), cons_obj());
    let ptr = v.as_heap_ptr().unwrap();
    store.teardown_all();
    let _ = store.region_of_ptr(ptr);
}

#[test]
fn foreign_store_page_is_not_generation_checked() {
    // Generations from two different stores are unrelated numbers
    // (docs/impl/region/generations.md § "Region generations"): a worker thread reading a
    // value allocated by its parent's heap must not compare the parent's
    // page stamp against its own counter. Store ids scope the check; a
    // foreign page resolves to its stamped region id exactly as before,
    // with no panic. (Reproduces the compile-primitives.lisp spawn-closure
    // false positive in miniature.)
    let mut a = RegionStore::default();
    let mut b = RegionStore::default();

    let ra = a.new_runtime_region();
    let va = a.alloc_obj(ra, cons_obj());
    let ptr = va.as_heap_ptr().unwrap();

    // B mints the same physical id (both stores start at 2), then frees it,
    // so B's current generation for the id differs from A's page stamp.
    let rb = b.new_runtime_region();
    assert_eq!(rb, ra, "both stores mint the same first id");
    b.alloc_obj(rb, cons_obj());
    b.decref(rb);
    assert_eq!(b.generation_raw(rb.get()), 1);

    // A's live page is stamped generation 0 by store A; checking it against
    // store B's counter (1) would false-positive without the store stamp.
    assert_eq!(b.region_of_ptr(ptr), ra.get());
}

#[test]
fn reclaimed_page_resolves_to_new_region_undetected() {
    // The documented boundary (docs/impl/region/generations.md § "Region generations"): once
    // the freed page is RE-CLAIMED by a new region the header is restamped,
    // so a stale pointer resolves — wrongly but self-consistently — to the
    // new region. That window belongs to --trace=guardfree (which never
    // re-claims pages). Pinned so a future widening of the check updates
    // the spec deliberately rather than by accident.
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    let v = store.alloc_obj(r, cons_obj());
    let ptr = v.as_heap_ptr().unwrap();
    store.decref(r);
    let r2 = store.new_runtime_region();
    let v2 = store.alloc_obj(r2, cons_obj()); // re-claims the cached page (LIFO)
    assert_eq!(
        v2.as_heap_ptr().unwrap(),
        ptr,
        "first slot of the re-claimed page is the old pointer (LIFO cache)"
    );
    assert_eq!(store.region_of_ptr(ptr), r2.get());
}
