//! Counterfactual tests for the uncounted cross-fiber borrow check
//! (docs/impl/region/generations.md § "Uncounted-borrow check").
//!
//! A child fiber snapshots heap values from its parent's dynamic-parameter
//! baseline without a reference count (the scheduler-via-parameter borrow). The
//! recorded `(region, generation)` lets the resume and `resolve_parameter`
//! checks confirm the borrowed region is still live — catching a dangling
//! borrow that a page-stamp check (`region_of`) misses once the freed page is
//! re-claimed and re-stamped. Region resolution and the generation read both go
//! through the one explicit heap passed in, so the recorded pair and the check
//! are within a single store.

use super::{first_stale_borrow, record_param_borrows};
use crate::hir::region::MappedRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{HeapObject, Pair};
use crate::value::Value;

/// The predicate that backs both check sites: a recorded borrow whose region
/// was freed reads stale (its generation moved), while a live one does not.
///
/// Counterfactual: without the recorded generation there is nothing to compare,
/// so a freed borrowed region is undetectable here — exactly the silent stale
/// read this check converts into a deterministic panic at the borrow.
#[test]
fn first_stale_borrow_detects_freed_region() {
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    let gen0 = heap.generation_raw(r.get());
    let borrows = vec![(7u32, r, gen0)];

    assert!(
        first_stale_borrow(&borrows, &heap).is_none(),
        "a borrow into a live region must not read stale",
    );

    // Free the borrowed region: its generation bumps, so the recorded snapshot
    // no longer matches — the borrow now dangles.
    heap.decref_region(r);
    assert_eq!(
        first_stale_borrow(&borrows, &heap),
        Some((7, r)),
        "a borrow into a freed region must read stale (generation moved)",
    );
}

/// Seeding records a borrow only for heap-valued bindings — an immediate
/// parameter value carries no region and is not a borrow. Region and generation
/// are read from the same explicit heap the value was allocated into.
#[test]
fn record_param_borrows_snapshots_heap_bindings_only() {
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();
    let v = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    // Param 7 borrows a heap value; param 9 is bound to an immediate.
    let flat = vec![(7u32, v), (9u32, Value::int(42))];

    let borrows = record_param_borrows(&flat, &heap);

    assert_eq!(
        borrows.len(),
        1,
        "only heap bindings are borrows; an immediate carries no region",
    );
    assert_eq!((borrows[0].0, borrows[0].1), (7, r));
    assert_eq!(
        borrows[0].2,
        heap.generation_raw(r.get()),
        "the live generation is recorded at seed",
    );
}

/// The suspended-frame borrow check: the recorded-generation analogue of the
/// param-snapshot one, for the uncounted region references a `BytecodeFrame`'s
/// `activation_region_map` holds across park/resume
/// (docs/impl/region/generations.md § "Two borrow shapes"). `record_region_borrows`
/// snapshots each `(slot, region, generation)` at suspend; the shared
/// `first_stale_borrow` flags any whose region's generation has since moved — a
/// region freed while the fiber was parked.
///
/// Counterfactual: without the recorded generation, a region freed (and possibly
/// recycled) while parked is invisible — `region_of` on the recycled page passes the
/// page-stamp check. The recorded generation catches it: the second assertion trips
/// where the first (live) does not.
#[test]
fn suspended_frame_region_borrow_detects_freed_region() {
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    // An activation_region_map mapping static region slot 7 to live physical
    // region r, tagged with r's generation at the moment the slot was established.
    let mut map: rustc_hash::FxHashMap<u32, MappedRegion> = rustc_hash::FxHashMap::default();
    map.insert(7u32, MappedRegion::new(r, heap.generation_raw(r.get())));
    let borrows = crate::value::fiber::record_region_borrows(&map, &heap);

    assert!(
        first_stale_borrow(&borrows, &heap).is_none(),
        "a suspended-frame borrow into a live region must not read stale",
    );

    // Free the region while the fiber is "parked": its generation bumps, so the
    // recorded snapshot no longer matches — the borrow now dangles.
    heap.decref_region(r);
    assert_eq!(
        first_stale_borrow(&borrows, &heap),
        Some((7, r)),
        "a suspended-frame borrow into a freed region must read stale (generation moved)",
    );
}

/// A map entry left dangling by a non-slot-clearing free (a value-based drop, a
/// cross-region cascade, a subtree drop) names a physical id that is then
/// recycled to an unrelated region. Such a **dead leftover** — its recorded
/// `MappedRegion::gen` no longer matches the id's current generation — must NOT
/// be snapshotted as a borrow: the activation never owned the recycled
/// incarnation, so recording it would forge a live borrow and trip the resume
/// check when that unrelated incarnation is freed (the stale-suspended-frame
/// false positive; docs/impl/region/generations.md § "Uncounted-borrow check").
///
/// Counterfactual: stamping the entry with the id's CURRENT generation (what the
/// pre-fix snapshot did) records `(slot, r, current_gen)`, and `first_stale_borrow`
/// then trips the instant the recycled incarnation is freed — a panic on a region
/// the parked activation never held. Recording the establish-generation and
/// skipping the mismatched entry is what makes the snapshot honest.
#[test]
fn stale_leftover_map_entry_is_not_snapshotted_as_a_borrow() {
    let mut heap = FiberHeap::new();

    // Establish slot 7 → physical id `r` at its establish generation.
    let r = heap.new_runtime_region();
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    let establish_gen = heap.generation_raw(r.get());
    let mut map: rustc_hash::FxHashMap<u32, MappedRegion> = rustc_hash::FxHashMap::default();
    map.insert(7u32, MappedRegion::new(r, establish_gen));

    // Free r by a path that does NOT clear the map slot, then recycle its
    // physical id to a fresh, unrelated region. The map still says slot 7 → r,
    // but that id now names a different incarnation (its generation moved).
    heap.decref_region(r);
    let recycled = heap.new_runtime_region();
    assert_eq!(
        recycled.get(),
        r.get(),
        "the freed physical id must be recycled for this test to exercise the leftover",
    );
    heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)),
        recycled,
    );
    assert_ne!(
        heap.generation_raw(r.get()),
        establish_gen,
        "recycling must have moved the generation past the establish generation",
    );

    // The dead leftover is skipped entirely — not recorded as a borrow.
    let borrows = crate::value::fiber::record_region_borrows(&map, &heap);
    assert!(
        borrows.is_empty(),
        "a leftover entry whose region's generation has moved is a dead mapping, \
         not a live borrow, and must not be snapshotted (got {borrows:?})",
    );

    // And freeing the recycled incarnation must NOT retroactively trip a borrow
    // check — the parked activation never borrowed it.
    heap.decref_region(recycled);
    assert!(
        first_stale_borrow(&borrows, &heap).is_none(),
        "freeing the unrelated recycled incarnation must not trip the check",
    );
}
