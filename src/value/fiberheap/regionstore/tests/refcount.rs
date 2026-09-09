// audited: 2026-09-08
//! Reference-count tests: what a decref frees, how far a cascade reaches, and
//! what a release that ran twice reports.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/diagnostics.md

use super::*;

#[test]
fn alloc_obj_creates_region_lazily() {
    let mut store = RegionStore::default();
    let v = store.alloc_obj(rr(5), cons_obj());
    assert!(v.is_heap());
    assert_eq!(store.region_obj_count(rr(5)), 1);
}

#[test]
fn alloc_region_slice_in_region() {
    let mut store = RegionStore::default();
    let s = store.alloc_region_slice(rr(3), b"hello");
    assert_eq!(s.as_slice(), b"hello");
}

#[test]
fn free_region_tears_down() {
    let mut store = RegionStore::default();
    for _ in 0..10 {
        store.alloc_obj(rr(4), cons_obj());
    }
    assert_eq!(store.region_obj_count(rr(4)), 10);
    // rc=0, decref frees immediately.
    store.decref(rr(4));
    assert_eq!(store.region_obj_count(rr(4)), 0);
}

#[test]
#[should_panic(expected = "DecrefRegion(99) but region was never alloc_in_region'd")]
fn decref_of_unallocated_region_panics_in_debug() {
    // Decref of a region id that was never alloc_in_region'd
    // is the "phantom region" class of bug — solver assigned a
    // region id to a node whose lowerer emits no alloc
    // instruction (DerefCell, MakeCell pre-fix; Eval without
    // call_result_regions registration). docs/impl/region/rules.md
    // § "Every region must correspond to a real allocation"
    // documents the rule; this debug_assert! catches violators
    // at the runtime boundary.
    let mut store = RegionStore::default();
    store.decref(rr(99)); // never allocated — debug build panics
}

#[test]
#[should_panic(expected = "DecrefRegion(4) but region was never alloc_in_region'd")]
fn double_decref_panics_in_debug() {
    // A region freed once must not be decref'd again. The
    // bytecode emitter must not produce two DecrefRegion(N)
    // instructions for the same N along the same path. The
    // saturating-arithmetic tolerance the data structure used
    // to provide hid bugs that the regions audit was exactly
    // chasing — replace tolerance with loud failure in debug.
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    store.decref(rr(4)); // rc=1 → 0, region freed, slot becomes None
    store.decref(rr(4)); // debug build panics on the second decref
}

// ── the over-free counter (docs/impl/region/diagnostics.md) ──────────────────
// The same violation the two `should_panic` tests above name, kept as a NUMBER
// for the build that compiles their `debug_assert!` out. The leak dashboards run
// in release, and no rate they carry can see a release that ran twice — the heap
// it leaves is smaller, not larger.
//
// These drive `decref_reaches_zero` directly rather than `decref`, because that
// is the only way to leave a counted region present at rc 0: `decref` frees such
// a region and clears its slot, so the second release would take the absent face
// and abort on the assert before the count could be read.

#[test]
fn a_direct_decref_of_a_zeroed_region_counts_an_over_free() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj()); // rc=1
    assert!(store.decref_reaches_zero(rr(4), None), "rc 1 → 0");
    assert_eq!(
        store.over_frees(),
        0,
        "the release that took the count to zero is the first one, not a second"
    );
    assert!(store.decref_reaches_zero(rr(4), None), "already at zero");
    assert_eq!(
        store.over_frees(),
        1,
        "a direct decref of an already-zeroed region is a release that ran twice"
    );
}

#[test]
fn a_cascade_decref_of_a_zeroed_region_is_not_an_over_free() {
    // One container's contents may name one region several times, so the free
    // cascade legitimately revisits a region an earlier visit already zeroed.
    // Counting that would make the gauge fire on correct programs.
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    assert!(store.decref_reaches_zero(rr(4), None), "rc 1 → 0");
    assert!(
        store.decref_reaches_zero(rr(4), Some(rr(3))),
        "cascade revisit"
    );
    assert_eq!(
        store.over_frees(),
        0,
        "a cascade revisit is not a violation"
    );
}

#[test]
fn a_cascade_decref_of_an_absent_region_is_not_an_over_free() {
    // The other cascade face: an earlier visit freed the region outright, so the
    // slot is gone by the time this one arrives.
    let mut store = RegionStore::default();
    assert!(!store.decref_reaches_zero(rr(99), Some(rr(3))));
    assert_eq!(store.over_frees(), 0);
}

#[test]
fn decref_if_present_skips_an_absent_slot_without_counting() {
    // The macro transient, the per-compilation transient and the embedding API
    // reserve a region id without necessarily allocating into it, so an absent
    // slot here is the documented pattern rather than a double-release.
    let mut store = RegionStore::default();
    assert_eq!(store.decref_if_present(rr(99)), 0);
    assert_eq!(store.over_frees(), 0);
}

#[test]
fn a_direct_decref_of_an_absent_region_counts_before_it_asserts() {
    // The face a release build reaches: with the `debug_assert!` compiled out the
    // decref returns false and the run carries on, so the count is the only
    // report left. The increment precedes the assert, so the two reports name the
    // same event and the counter is already right at the moment of the abort.
    //
    // Caught rather than `should_panic` because the count is what is under test
    // and a `should_panic` test cannot read anything afterwards. The panic
    // message reaching stderr is the cost.
    let mut store = RegionStore::default();
    {
        let caught = std::panic::AssertUnwindSafe(&mut store);
        let _ = std::panic::catch_unwind(move || {
            let caught = caught;
            caught.0.decref_reaches_zero(rr(99), None)
        });
    }
    assert_eq!(
        store.over_frees(),
        1,
        "a direct decref of a region with no entry is a release that ran twice"
    );
}

#[test]
fn rc_prevents_free() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj()); // rc=1 (scope ref)
    store.incref(rr(4)); // rc=2 (simulate cross-region ref)
    assert_eq!(store.rc(rr(4)), 2);

    // FreeRegion (decref): rc 2→1, not freed (cross-ref holds it).
    store.decref(rr(4));
    assert_eq!(store.rc(rr(4)), 1);
    assert_eq!(
        store.region_obj_count(rr(4)),
        1,
        "region not freed while rc > 0"
    );

    // Cascade decref: rc 1→0, freed.
    store.decref(rr(4));
    assert_eq!(
        store.region_obj_count(rr(4)),
        0,
        "region freed when rc reaches 0"
    );
}

#[test]
fn incref_decref_basic() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(7), cons_obj()); // rc=1 (scope ref)
    store.incref(rr(7)); // rc=2
    assert_eq!(store.rc(rr(7)), 2);
    store.decref(rr(7)); // rc=1
    assert_eq!(store.rc(rr(7)), 1);
    store.decref(rr(7)); // rc=0, freed
    assert_eq!(store.rc(rr(7)), 0);
}

#[test]
fn decref_at_zero_frees() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    assert_eq!(store.region_obj_count(rr(4)), 1);
    store.decref(rr(4));
    assert_eq!(store.region_obj_count(rr(4)), 0);
}

#[test]
fn total_obj_count_across_regions() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    store.alloc_obj(rr(4), cons_obj());
    store.alloc_obj(rr(2), cons_obj());
    assert_eq!(store.total_obj_count(), 3);
}

#[test]
fn teardown_all_clears_everything() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    store.alloc_obj(rr(2), cons_obj());
    store.alloc_obj(rr(3), cons_obj());
    store.teardown_all();
    assert_eq!(store.total_obj_count(), 0);
}

#[test]
fn owns_detects_region_pointers() {
    let mut store = RegionStore::default();
    let v = store.alloc_obj(rr(4), cons_obj());
    let ptr = v.as_heap_ptr().unwrap();
    assert!(store.owns(ptr));

    let x: i64 = 42;
    assert!(!store.owns(&x as *const _ as *const ()));
}

#[test]
fn dtors_run_on_free() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut store = RegionStore::default();
    let cell = Rc::new(RefCell::new(Value::NIL));
    let weak = Rc::downgrade(&cell);
    store.alloc_obj(
        rr(4),
        HeapObject::LBox {
            cell,
            traits: Value::NIL,
        },
    );
    store.decref(rr(4));
    assert!(
        weak.upgrade().is_none(),
        "Rc should be dropped when region is freed"
    );
}

#[test]
fn multiple_regions_independent() {
    let mut store = RegionStore::default();
    store.alloc_obj(rr(4), cons_obj());
    store.alloc_obj(rr(4), cons_obj());
    store.alloc_obj(rr(2), cons_obj());

    store.decref(rr(4));
    assert_eq!(store.region_obj_count(rr(4)), 0);
    assert_eq!(
        store.region_obj_count(rr(2)),
        1,
        "region 2 should be unaffected"
    );
}

#[test]
fn cascade_decrefs_cross_region_refs() {
    // Region 2 has a value (rc=1). Region 3 has an @array with that value.
    // alloc_obj auto-increfs region 2 for the cross-region ref → rc(2)=2.
    let mut store = RegionStore::default();
    let val_in_r2 = store.alloc_obj(rr(2), cons_obj()); // rc(2)=1

    let arr = HeapObject::LArrayMut {
        data: std::rc::Rc::new(std::cell::RefCell::new(vec![val_in_r2])),
        traits: Value::NIL,
    };
    store.alloc_obj(rr(3), arr); // auto-incref r2 → rc(2)=2

    assert_eq!(store.rc(rr(2)), 2);

    // Free region 3 — cascade decrefs region 2 → rc(2)=1 (scope ref remains).
    store.decref(rr(3));
    assert_eq!(store.region_obj_count(rr(3)), 0, "region 3 should be freed");
    assert_eq!(
        store.rc(rr(2)),
        1,
        "cascade decrefs cross-region ref, scope ref remains"
    );
}

#[test]
fn free_region_decrefs_escaped() {
    // Region 2 value held by @array in region 3. auto-incref → rc(2)=2.
    // FreeRegion(2) decrefs to 1 (cross-ref holds it).
    // Free r3 → cascade decrefs r2 → rc=0, freed.
    let mut store = RegionStore::default();
    let val_in_r2 = store.alloc_obj(rr(2), cons_obj()); // rc(2)=1

    let arr = HeapObject::LArrayMut {
        data: std::rc::Rc::new(std::cell::RefCell::new(vec![val_in_r2])),
        traits: Value::NIL,
    };
    store.alloc_obj(rr(3), arr); // auto-incref r2 → rc(2)=2

    assert_eq!(store.rc(rr(2)), 2);

    // FreeRegion(2): rc 2→1, not freed (cross-ref holds it).
    store.decref(rr(2));
    assert_eq!(store.rc(rr(2)), 1);
    assert_eq!(
        store.region_obj_count(rr(2)),
        1,
        "region 2 held by cross-ref from r3"
    );

    // Free r3 → cascade decrefs r2 → rc=0, freed.
    store.decref(rr(3));
    assert_eq!(
        store.region_obj_count(rr(2)),
        0,
        "region 2 freed after cascade from r3"
    );
}

#[test]
fn cascade_box_cross_region() {
    let mut store = RegionStore::default();
    let val_in_r2 = store.alloc_obj(rr(2), cons_obj()); // rc(2)=1

    let bx = HeapObject::LBox {
        cell: std::rc::Rc::new(std::cell::RefCell::new(val_in_r2)),
        traits: Value::NIL,
    };
    store.alloc_obj(rr(3), bx); // auto-incref r2 → rc(2)=2

    store.decref(rr(3)); // cascade: rc(2)=1
    assert_eq!(
        store.rc(rr(2)),
        1,
        "cascade should decref box's cross-region ref"
    );
}

#[test]
fn deep_cascade_chain_does_not_overflow_stack() {
    // A linear chain of N regions: region k holds an @array containing region
    // (k+1)'s value, so freeing the head must cascade-free the whole chain.
    // The free cascade is driven by an explicit worklist (free.rs
    // `free_region_set`), NOT native recursion — otherwise a chain of a few
    // thousand links overflows the stack. This is the region-store shape behind
    // the `(apply concat <thousands-of-chunks>)` and deep-list/nested-structure
    // programs (tests/elle/region-deep-chain.lisp): each link is one more
    // frontier decref, and a recursive cascade spends one stack frame per link.
    //
    // N is chosen well past a worker thread's default stack: a recursive
    // cascade dies here, an iterative one frees in O(1) stack.
    const N: u32 = 40_000;
    let mut store = RegionStore::default();
    let leaf = |s: &mut RegionStore, id: u32| s.alloc_obj(rr(id), cons_obj());
    let link = |s: &mut RegionStore, id: u32, child: Value| {
        s.alloc_obj(
            rr(id),
            HeapObject::LArrayMut {
                data: std::rc::Rc::new(std::cell::RefCell::new(vec![child])),
                traits: Value::NIL,
            },
        )
    };
    // Region ids 2..=N+1 (0/1 are reserved). Tail (N+1) is the leaf; each region
    // k<N+1 holds region k+1's value, auto-increfing k+1 to rc=2 (scope + the
    // one incoming cross-ref). The head (region 2) keeps rc=1.
    let mut child = leaf(&mut store, N + 1);
    for k in (2..=N).rev() {
        child = link(&mut store, k, child);
    }
    // Drop every non-head region's scope reference (rc 2→1) so each is held
    // solely by its predecessor's cross-ref; none frees yet.
    for k in 3..=(N + 1) {
        store.decref(rr(k));
        assert_eq!(store.rc(rr(k)), 1, "region {k} still held by its parent");
    }
    assert_eq!(store.total_obj_count() as u32, N, "chain fully built");
    // Free the head: rc 1→0 triggers the whole-chain cascade in one call.
    store.decref(rr(2));
    assert_eq!(
        store.total_obj_count(),
        0,
        "entire chain reclaimed by the head free's cascade"
    );
}

#[test]
fn cascade_struct_mut_cross_region() {
    let mut store = RegionStore::default();
    let val_in_r2 = store.alloc_obj(rr(2), cons_obj()); // rc(2)=1

    let mut map = std::collections::BTreeMap::new();
    map.insert(
        crate::value::TableKey::from_value(&Value::keyword("x")).unwrap(),
        val_in_r2,
    );
    let sm = HeapObject::LStructMut {
        data: std::rc::Rc::new(std::cell::RefCell::new(map)),
        traits: Value::NIL,
    };
    store.alloc_obj(rr(3), sm); // auto-incref r2 → rc(2)=2

    store.decref(rr(3)); // cascade: rc(2)=1
    assert_eq!(
        store.rc(rr(2)),
        1,
        "cascade should decref @struct's cross-region ref"
    );
}
