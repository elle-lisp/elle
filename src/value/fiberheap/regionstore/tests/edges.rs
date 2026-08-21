use super::*;

// ── The outgoing edge table ──────────────────────────────────────────────────
//
// docs/impl/region/ownership.md § "The outgoing edge table". Every cross-region
// CONTENT edge — a `Value` in a region's heap objects pointing into another
// region — is recorded at creation into the source region's `outgoing` table, so
// reclamation walks the table (O(edges)) instead of scanning page contents. The
// invariant these pins hold to: the recorded table equals what the content scan
// (`find_object_cross_refs`, surfaced as `cross_ref_edges`) would find. Each is a
// counterfactual against the pre-step-0 state (no table, scan-only free) — RED
// while `outgoing` is unpopulated, GREEN once the funnels record into it.

/// The alloc funnel records the content edge it increfs: an array in region `h`
/// holding a value from region `c` records `h → c`, and that recorded edge equals
/// what the content scan independently finds. RED before the creation funnel
/// records (the table is empty while the scan reports the edge).
#[test]
fn recorded_outgoing_matches_scan_after_alloc() {
    let mut store = RegionStore::default();
    let c = store.new_runtime_region();
    let h = store.new_runtime_region();
    let cv = store.alloc_obj(c, cons_obj());
    store.alloc_obj(
        h,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![cv])),
            traits: Value::NIL,
        },
    );
    assert_eq!(
        store.outgoing_edges(h),
        vec![(c.get(), 1)],
        "the alloc funnel records the content edge h → c"
    );
    let scan_for_h: Vec<(u32, u32)> = store
        .cross_ref_edges()
        .into_iter()
        .filter(|&(from, _)| from == h.get())
        .collect();
    assert_eq!(
        scan_for_h,
        vec![(h.get(), c.get())],
        "the content scan independently finds the same edge (the oracle's invariant)"
    );
}

/// Filter parity: a value stored into a container in its OWN region is a self-edge
/// the content scan filters (`rid == own_id`), so the recorded table must skip it
/// too — no edge. Pins the recorder's self-edge filter against a wrong impl that
/// records `r → r`.
#[test]
fn same_region_store_records_no_edge() {
    let mut store = RegionStore::default();
    let r = store.new_runtime_region();
    let v = store.alloc_obj(r, cons_obj());
    store.alloc_obj(
        r,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![v])),
            traits: Value::NIL,
        },
    );
    assert!(
        store.outgoing_edges(r).is_empty(),
        "a same-region store records no edge (self-edge filtered, scan parity)"
    );
}

/// Universality: the outgoing table is present on `Owned` regions too — an Owned
/// region carries it for the cascade-on-drop even though it has no RC — and
/// adoption (Counted → Owned) must leave it intact. RED before the funnel records;
/// also guards against an adoption that clears `outgoing`.
#[test]
fn owned_region_carries_outgoing_but_no_count() {
    let mut store = RegionStore::default();
    let owner = store.new_runtime_region();
    let c = store.new_runtime_region();
    let h = store.new_runtime_region();
    let cv = store.alloc_obj(c, cons_obj());
    store.alloc_obj(
        h,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![cv])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(owner, cons_obj());
    store.adopt_region(owner, h); // h: Counted → Owned
    assert_eq!(store.rc(h), 0, "an Owned region has no independent count");
    assert_eq!(
        store.outgoing_edges(h),
        vec![(c.get(), 1)],
        "the Owned region still carries its outgoing edge (cascade-on-drop is universal)"
    );
}

/// Ownership parity: a `Value` living on ANOTHER store's heap — a
/// compile-time-env constant baked into a closure template's pool, a worker
/// reading a parent-heap value — contributes NO edge on either side.
/// `find_object_cross_refs` resolves a pointer by reading its masked page
/// header, and a foreign page's bytes can spell ANY id, including one that is
/// (or later becomes) a live region of this store; liveness alone is therefore
/// time-dependent, and an id dead at alloc-record time but live at free-scan
/// time would split the recorded table from the scan — the drift the oracle
/// detonates on. The ownership predicate (`RegionPool::owns`) is
/// time-invariant for a foreign pointer — no local region ever owns its
/// address — so record and scan agree by construction: no edge recorded here,
/// and the free below is oracle-clean.
#[test]
fn foreign_store_value_records_no_edge_and_frees_clean() {
    // The foreign value's page header is stamped with the FOREIGN store's
    // region id — its first mint, id 2. Mint a LIVE local region with that
    // same id first (and hold the host in a different one), so a liveness-only
    // filter reads the foreign header as a live local region and records a
    // spurious edge — the deterministic collision this pin forbids.
    let mut foreign = RegionStore::default();
    let fr = foreign.new_runtime_region();
    let fv = foreign.alloc_obj(fr, cons_obj());

    let mut store = RegionStore::default();
    let collide = store.new_runtime_region();
    assert_eq!(
        collide.get(),
        fr.get(),
        "precondition: the local decoy shares the foreign value's stamped id"
    );
    store.alloc_obj(collide, cons_obj());
    let r = store.new_runtime_region();
    store.alloc_obj(
        r,
        HeapObject::Pair(crate::value::heap::Pair::new(fv, Value::NIL)),
    );
    assert!(
        store.outgoing_edges(r).is_empty(),
        "a foreign-store value must record no outgoing edge (ownership parity): \
         no live local region owns its address, whatever id its foreign page \
         header spells"
    );
    store.decref(r); // oracle-clean: the scan skips the foreign pointer too
}

/// The free-time equivalence oracle's teeth: at free the recorded table must match
/// a content scan of the freed members. A spurious edge `h → x` absent from `h`'s
/// contents must detonate when `h` frees (table has `x`, scan does not). RED before
/// the oracle exists (the free silently succeeds, so `#[should_panic]` fails);
/// GREEN once the oracle asserts table == scan. Debug-only (the oracle is
/// `#[cfg(debug_assertions)]`).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "outgoing")]
fn oracle_panics_on_drift() {
    let mut store = RegionStore::default();
    let c = store.new_runtime_region();
    let x = store.new_runtime_region();
    let h = store.new_runtime_region();
    let cv = store.alloc_obj(c, cons_obj());
    store.alloc_obj(
        h,
        HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![cv])),
            traits: Value::NIL,
        },
    );
    store.alloc_obj(x, cons_obj()); // a valid frontier region, NOT in h's contents
    store.force_outgoing_edge_for_test(h, x); // drift: table {c, x}, content {c}
    store.decref(h); // h rc 1 → 0 → free → oracle compares table vs scan → panic
}
