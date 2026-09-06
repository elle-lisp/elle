// audited: 2026-09-05
//! Counter-factual tests for the two uncounted region borrows that carry a
//! recorded generation, and for the panic a stale one raises.
//!
//! docs/impl/region/generations.md
//!
//! Region resolution and the generation read both go through the one explicit
//! heap passed in, so the recorded pair and the check are within a single store.

use super::{first_stale_borrow, record_param_borrows};
use crate::hir::region::MappedRegion;
use crate::reader::SourceLoc;
use crate::value::fiber::ParkSite;
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
    let borrows = crate::value::fiber::record_region_borrows(&map, &heap, &[7]);

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
    let borrows = crate::value::fiber::record_region_borrows(&map, &heap, &[7]);
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

/// A map slot the function never releases by id is not a borrow, however live
/// its region still is (docs/impl/region/generations.md).
///
/// The trap: the establish-generation separates a dead leftover from a live
/// borrow only once the region has been freed. At park a leftover whose region
/// is still live reads exactly like a borrow — the free that would move the
/// generation has not happened yet — and it is the free AFTER the park that the
/// resume check then reports. The heap alone cannot tell the two apart, so the
/// snapshot asks the function which slots its slot-routed releases name.
///
/// Counter-factual: drop the `slot_routed` argument and record on generation
/// alone, and the first assertion below records slot 7 — a slot whose region is
/// released by value, so no `DecrefRegion` ever reads the entry. Freeing that
/// region while the fiber is parked then trips the resume check on a mapping the
/// activation reads through nothing.
#[test]
fn a_slot_with_no_slot_routed_release_is_not_a_borrow() {
    let mut heap = FiberHeap::new();

    // Two live slots, both mapped at their establish generation. The function
    // releases slot 9 by id; slot 7's region is value-routed, so slot 7 appears
    // in no `DecrefRegion` this function emits.
    let by_value = heap.new_runtime_region();
    let by_slot = heap.new_runtime_region();
    for r in [by_value, by_slot] {
        heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)), r);
    }
    let mut map: rustc_hash::FxHashMap<u32, MappedRegion> = rustc_hash::FxHashMap::default();
    map.insert(
        7u32,
        MappedRegion::new(by_value, heap.generation_raw(by_value.get())),
    );
    map.insert(
        9u32,
        MappedRegion::new(by_slot, heap.generation_raw(by_slot.get())),
    );

    let borrows = crate::value::fiber::record_region_borrows(&map, &heap, &[9]);

    assert_eq!(
        borrows.len(),
        1,
        "only the slot-routed mapping is a borrow; a value-routed slot holds no \
         pending DecrefRegion (got {borrows:?})",
    );
    assert_eq!((borrows[0].0, borrows[0].1), (9, by_slot));

    // Freeing the value-routed region while "parked" must not read stale: the
    // activation reaches that entry through no release.
    heap.decref_region(by_value);
    assert!(
        first_stale_borrow(&borrows, &heap).is_none(),
        "freeing a region whose slot the function never releases by id must not \
         trip the check",
    );

    // The slot-routed one still does, so the filter has not blinded the check.
    heap.decref_region(by_slot);
    assert_eq!(
        first_stale_borrow(&borrows, &heap),
        Some((9, by_slot)),
        "a slot-routed borrow freed while parked must still read stale",
    );
}

/// The resume-boundary panic names the activation that parked, not just the
/// slot and the physical region (docs/impl/region/generations.md).
///
/// The trap: a slot number and a physical region id are per-run values. Both
/// change with the batch, the build profile and the I/O backend, so a panic
/// carrying only those cannot be traced back to any line of any program — which
/// is the whole difficulty of a failure that reproduces only on a machine the
/// reader cannot run.
///
/// Counter-factual: assert only that the message holds the slot and the region,
/// and the pre-existing message passes unchanged. The function name and the
/// source location are what the assertions below add.
#[test]
fn stale_borrow_message_names_the_parked_site() {
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();

    let site = ParkSite {
        function: Some("drain-body"),
        at: Some(SourceLoc::new("tests/elle/http.lisp", 214, 7)),
        start: Some(SourceLoc::new("tests/elle/http.lisp", 200, 1)),
        ip: 61,
        frame: 1,
        frames: 3,
    };
    let msg = site.stale_borrow_message(136968, r);

    assert!(
        msg.contains("136968") && msg.contains(&format!("{}", r.get())),
        "the slot and the physical region must survive the rewrite: {msg}",
    );
    assert!(
        msg.contains("drain-body"),
        "the parked activation's function must be named: {msg}",
    );
    assert!(
        msg.contains("tests/elle/http.lisp:214:7"),
        "the resume point's own source location must be named: {msg}",
    );
    assert!(
        msg.contains("frame 1 of 3"),
        "the frame's position in the replay chain must be named: {msg}",
    );
}

/// Where the resume ip has no location entry of its own, the message falls back
/// to the function's first recorded line. Naming the file is most of the answer,
/// and an exact-match table lookup misses whenever the resume point is not
/// itself a recorded offset.
///
/// Counter-factual: report `at` alone and this case prints no location at all —
/// the reader learns the region id and nothing about which program parked.
#[test]
fn stale_borrow_message_falls_back_to_the_function_start() {
    let mut heap = FiberHeap::new();
    let r = heap.new_runtime_region();

    let site = ParkSite {
        function: None,
        at: None,
        start: Some(SourceLoc::new("lib/http2.lisp", 88, 3)),
        ip: 61,
        frame: 0,
        frames: 1,
    };
    let msg = site.stale_borrow_message(7, r);

    assert!(
        msg.contains("lib/http2.lisp:88:3"),
        "the function's first recorded line must stand in for a missing entry: {msg}",
    );
    assert!(
        msg.contains("<anonymous>"),
        "an unnamed activation must still be reported as one: {msg}",
    );
}
