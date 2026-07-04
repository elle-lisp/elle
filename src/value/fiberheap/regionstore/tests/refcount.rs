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
    // call_result_regions registration). docs/impl/region-rules.md
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
