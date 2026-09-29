// audited: 2026-09-29
//! Which parks owe their payload a release at an install, and which must be left to the resumed body.
//!
//! docs/impl/region/park.md

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::fiber::Delivery;
use crate::value::heap::{HeapObject, Pair};
use crate::value::{Closure, Fiber, FiberHandle, SIG_IO, SIG_YIELD};
use std::rc::Rc;

fn cons() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
}

/// A pair on a region of its own, standing in for any parked payload.
fn payload(
    heap: &mut crate::value::fiberheap::FiberHeap,
) -> (Value, crate::hir::region::RuntimeRegion) {
    alloc_in_fresh_region(heap, cons())
}

/// An `IoRequest` on a region of its own — the payload a yielding io op parks,
/// and the one a relaying fiber parks again.
fn io_request(
    heap: &mut crate::value::fiberheap::FiberHeap,
) -> (Value, crate::hir::region::RuntimeRegion) {
    let region = heap.new_runtime_region();
    let request = crate::io::request::IoRequest::test_sleep(
        &crate::primitives::ctx::Alloc::with_region(region, heap),
    );
    (request, region)
}

/// A fiber whose body never runs still names a code object; the instance's
/// placeholder is exactly that. The heap is leaked so the placeholder stays
/// resident for the test.
fn test_closure() -> Rc<Closure> {
    let heap = crate::value::arena::leaked_test_heap();
    crate::value::fiber::noop_closure(unsafe { &mut *heap })
}

/// A fiber parked on `parked` under `bits`, whose ledger `park` writes as the
/// site that built the park would.
fn fiber_parked(bits: SignalBits, parked: Value, park: impl FnOnce(&mut Delivery)) -> FiberHandle {
    let handle = FiberHandle::new(Fiber::new(test_closure(), SignalBits::ALL));
    handle.with_mut(|f| {
        f.signal = Some((bits, parked));
        park(&mut f.delivery);
    });
    handle
}

// -- release_displaced_bodyless_payload: the record names what the install owes --

/// A capability denial's park has no body reference, so the install that
/// displaces it releases the one the payload is left with. The blocked bits are
/// NOT `SIG_IO` here: a bits-only gate would let a denial of any other capability
/// through, stranding the payload's region once per mediation.
#[test]
fn a_recorded_denial_park_is_released_by_the_install() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = fiber_parked(SIG_YIELD, p, |d| d.park_denial(SIG_YIELD, p));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a mediated denial's payload region owes exactly one decref at the install",
    );
}

/// The record is matched against the LIVE parked signal, so an install reaching a
/// fiber whose denial park is already over releases nothing. Counter-factual:
/// releasing on the record alone would decref a region this install never owed,
/// once per stale record left on a fiber that parked again.
#[test]
fn a_record_that_no_longer_names_the_parked_signal_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (parked, rid) = payload(&mut heap);
    let (stale, _) = payload(&mut heap);
    let handle = fiber_parked(SIG_YIELD, parked, |d| d.park_denial(SIG_YIELD, stale));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the decref is owed by the park the record names, not by whatever \
         occupies the signal slot later",
    );
}

/// The counter-factual for the gate itself: an `(emit :fs v)` parks a
/// body-allocated payload under the very bits a `:fs` denial parks under. Nothing
/// records it, so nothing releases it — the resumed body's own continuation does,
/// and a decref here would free the value under every holder that outlives the
/// fiber.
#[test]
fn an_unrecorded_park_under_the_same_bits_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = fiber_parked(SIG_YIELD, p, |_| {});
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "a body-owned park owes the install nothing",
    );
}

/// Taking the record IS the receipt. Five installs run this and a denied fiber
/// can reach more than one of them — `fiber/refuse` after a `fiber/resume` that
/// re-parked, the `protect` route's inner delivery ahead of the outer resume.
/// Counter-factual: a gate that only compared, leaving the record in place, would
/// release the same reference once per install and free the payload under the
/// mediator still reading it.
#[test]
fn the_record_is_taken_so_a_second_install_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = fiber_parked(SIG_YIELD, p, |d| d.park_denial(SIG_YIELD, p));

    release_displaced_bodyless_payload(&mut heap, &handle);
    let after_first = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        after_first,
        "one reference is owed per park, however many installs displace it",
    );
}

/// A fiber denied `:io` parks its denial struct under `SIG_IO`, the bit an io
/// op's request parks under. The record names the struct, and one reference is
/// owed. Counter-factual: a second reading keyed on the bit releases the struct
/// again and frees it under the mediator.
#[test]
fn an_io_denial_owes_the_install_one_release() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = fiber_parked(SIG_IO, p, |d| d.park_denial(SIG_IO, p));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a denial under the io bit owes ONE reference, and it is the record's",
    );
}

// -- release_displaced_bodyless_payload: an io op's request, and a relay's --

/// An io op's request is the runtime's value, so whatever ends the park owes the
/// reference the allocation left. The injection `fiber/abort` and
/// `fiber/refuse` share reaches this release with no resume value at all.
#[test]
fn an_io_op_park_is_released_by_the_install() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let handle = fiber_parked(SIG_IO, request, |d| d.park_request(SIG_IO, request));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "the install that displaces an io park owes its request one decref",
    );
}

/// A fiber that relays a child's io park with `(emit :io v)` parks the child's
/// request under `SIG_IO`, so the bits and the payload's type are an io op's.
/// The relay is an `Emit` park of a borrowed value, whose body owns a
/// reference, so the install that answers it owes the request nothing.
/// Counter-factual: a reading keyed on the bit and the type releases the child's
/// region once per relaying fiber, and frees the child's port and buffers under
/// it (`tests/impl/region-io-relay-uaf.lisp`).
#[test]
fn a_relayed_io_request_owes_the_relaying_install_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let handle = fiber_parked(SIG_IO, request, |d| d.park_emit(SIG_IO, request));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "a relay's request is body-owned — the relaying install owes it nothing",
    );
}

/// The same relay through the `emit` primitive, whose keyword the compiler
/// cannot read: a primitive park of the call's own argument, which records no
/// payload to release.
#[test]
fn a_dynamic_emit_of_a_request_owes_the_install_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let handle = fiber_parked(SIG_IO, request, |d| d.park_primitive(SIG_IO, request));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "a dynamic emit's payload is body-owned like a literal emit's",
    );
}

/// A fiber that only passes an io park on — the outer fiber of a `protect`ed
/// body — holds the request in its slot and nothing in its ledger. The release
/// is owed at the install on the fiber that parked. Counter-factual: releasing
/// here as well releases the request once per fiber it crosses.
#[test]
fn a_fiber_that_passes_an_io_park_on_owes_its_install_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let handle = fiber_parked(SIG_IO, request, |_| {});
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the release belongs to the install on the fiber whose ledger names it",
    );
}

/// The trap: the install asks the ledger, never the payload. A park no record
/// names may hold a value whose region another holder already released, and
/// reading it is a stale deref — the generation stamp's panic in a debug build.
#[test]
fn an_unrecorded_park_is_answered_without_dereferencing_its_payload() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    crate::value::arena::decref_region(&mut heap, Some(rid));
    let handle = fiber_parked(SIG_IO, p, |_| {});

    release_displaced_bodyless_payload(&mut heap, &handle);
}

// -- release_abandoned_park: the boundary owes what no seam consumed --

/// A boundary ends a park with no reader and no install, so both of the park's
/// references are its to release: the delivery retain the park took, and the one
/// the runtime's own allocation left. Counter-factual: releasing the delivery
/// alone leaves the request's region at rc 1 for good, one region and one object
/// per squelched io op.
#[test]
fn a_boundary_releases_both_of_an_io_parks_references() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let mut delivery = Delivery::new();
    let live = Some((SIG_IO, request));
    delivery.park_request(SIG_IO, request);
    let before = region_rc(&heap, rid);

    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(
        region_rc(&heap, rid),
        before - 2,
        "the allocation's reference and the delivery retain are both owed here",
    );
}

/// A body-allocated payload owes the boundary ONE reference. Its own is the
/// abandoned frames' release tables' to run, so a second decref here would free
/// the value under the frame whose table is about to name it.
#[test]
fn a_boundary_releases_one_reference_of_a_body_allocated_park() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let mut delivery = Delivery::new();
    let live = Some((SIG_YIELD, p));
    delivery.park_emit(SIG_YIELD, p);
    let before = region_rc(&heap, rid);

    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "only the delivery retain has no consumer — the body's reference has one",
    );
}

/// A relay's `(emit :io v)` squelched at a boundary is a body-owned park under
/// the io bit. Its request owes the boundary the delivery alone; the relaying
/// body's reference is its frames' to release. Counter-factual: a reading keyed
/// on the bit and the type releases it twice.
#[test]
fn a_boundary_releases_one_reference_of_a_relayed_io_park() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let mut delivery = Delivery::new();
    let live = Some((SIG_IO, request));
    delivery.park_emit(SIG_IO, request);
    let before = region_rc(&heap, rid);

    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a relayed request's only unconsumed reference is the delivery retain",
    );
}

/// A denial park is runtime-built like an io request, and its second reference
/// is named by the same ledger record an io op's request writes.
#[test]
fn a_boundary_releases_both_of_a_denial_parks_references() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let mut delivery = Delivery::new();
    let live = Some((SIG_IO, p));
    delivery.park_denial(SIG_IO, p);
    let before = region_rc(&heap, rid);

    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(
        region_rc(&heap, rid),
        before - 2,
        "a denial's struct has no body reference either — the record names it",
    );
}

/// The receipt: taking the record is what keeps a second boundary over the same
/// fiber from releasing the same references again.
#[test]
fn a_second_boundary_over_the_same_fiber_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let mut delivery = Delivery::new();
    let live = Some((SIG_IO, request));
    delivery.park_request(SIG_IO, request);

    release_abandoned_park(&mut heap, &mut delivery, live);
    let after_first = region_rc(&heap, rid);
    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(
        region_rc(&heap, rid),
        after_first,
        "one park, one set of releases, however many boundaries the fiber meets",
    );
}

/// A boundary that ends no park releases nothing. This is the ordinary case —
/// most violations catch a raise rather than a suspension.
#[test]
fn a_boundary_with_no_park_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let live = Some((SIG_YIELD, p));
    let mut delivery = Delivery::new();
    let before = region_rc(&heap, rid);

    release_abandoned_park(&mut heap, &mut delivery, live);

    assert_eq!(region_rc(&heap, rid), before);
    assert!(p.as_heap_ptr().is_some(), "the payload is untouched");
}

/// The gate the ledger alone cannot supply: a record left over from a park some
/// other route already ended names a value this exit is not looking at, and
/// releasing it would drop a reference that park's own end already consumed.
/// A host that refuses a suspension it cannot resume leaves exactly that
/// (`VM::abandon_hosted_park`'s subject), and no route-completeness argument is
/// needed once the two are compared.
#[test]
fn a_record_that_does_not_name_the_exits_park_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (stale, stale_rid) = payload(&mut heap);
    let (live_payload, _) = payload(&mut heap);
    let mut delivery = Delivery::new();
    delivery.park_emit(SIG_YIELD, stale);
    let before = region_rc(&heap, stale_rid);

    release_abandoned_park(&mut heap, &mut delivery, Some((SIG_YIELD, live_payload)));

    assert_eq!(
        region_rc(&heap, stale_rid),
        before,
        "the decref is owed by the park the exit is ending, not by whatever the \
         ledger last recorded",
    );
}

// -- the shared region exempts no install --

/// A `Fresh` io op (`port/read`, `accept`) builds its completion buffer in the
/// request's OWN region and hands that buffer back as the resume value. A second
/// value on the region is not a second consumer of the suspend retain: the
/// buffer's holders release what they took, and the install is the retain's only
/// consumer. An install that stands down on the shared region leaves the retain
/// standing for good — the region survives with its buffer and its request, once
/// per read (`tests/impl/region-io-read-strand.lisp` bounds the rate).
///
/// The second reference below stands for the resume value's own holder, which is
/// what makes the release safe: it drops the retain, not the buffer.
#[test]
fn an_io_park_is_released_though_its_resume_value_shares_its_region() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let _completion = heap.alloc_in_region(cons(), rid);
    crate::value::arena::incref_region(&mut heap, Some(rid));
    let handle = fiber_parked(SIG_IO, request, |d| d.park_request(SIG_IO, request));
    let before = region_rc(&heap, rid);

    release_displaced_bodyless_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a resume value sharing the request's region still owes the suspend retain",
    );
    assert!(
        region_rc(&heap, rid) > 0,
        "the release drops the suspend retain, not the buffer the resume hands back",
    );
}
