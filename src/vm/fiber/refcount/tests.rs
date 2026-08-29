//! Which parks owe their payload a release at an install, and which must be
//! left to the resumed body.

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
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

/// A resume value on a region of its own — what a mediating parent hands back,
/// which never shares the denial payload's region.
fn foreign(heap: &mut crate::value::fiberheap::FiberHeap) -> Value {
    alloc_in_fresh_region(heap, cons()).0
}

fn test_closure() -> Rc<Closure> {
    use crate::value::types::Arity;
    use crate::value::ClosureTemplate;
    Rc::new(Closure {
        template: crate::value::TemplateRef::new(Rc::new(ClosureTemplate::new(
            Rc::new(vec![]),
            Arity::Exact(0),
            Rc::new(vec![]),
        ))),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    })
}

/// A fiber parked on `parked` under `bits`, with `record` as its denial record.
/// The blocked bits of a real denial are the WITHHELD capability's, which is why
/// they are not `SIG_IO` in most faces below.
fn parked_fiber(bits: SignalBits, parked: Value, record: Option<Value>) -> FiberHandle {
    let handle = FiberHandle::new(Fiber::new(test_closure(), SignalBits::ALL));
    handle.with_mut(|f| {
        f.signal = Some((bits, parked));
        f.denial_payload = record;
    });
    handle
}

// -- release_displaced_denial_payload: the record names what the install owes --

/// A capability denial's park has no body reference, so the install that
/// displaces it releases the one the payload is left with. The blocked bits are
/// NOT `SIG_IO` here: a bits-only gate would let a denial of any other capability
/// through, stranding the payload's region once per mediation.
#[test]
fn a_recorded_denial_park_is_released_by_the_install() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = parked_fiber(SIG_YIELD, p, Some(p));
    let before = region_rc(&heap, rid);

    let claimed = release_displaced_denial_payload(&mut heap, &handle);

    assert!(claimed, "the record claims the park it names");
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
    let handle = parked_fiber(SIG_YIELD, parked, Some(stale));
    let before = region_rc(&heap, rid);

    let claimed = release_displaced_denial_payload(&mut heap, &handle);

    assert!(!claimed, "a stale record claims nothing");
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
    let handle = parked_fiber(SIG_YIELD, p, None);
    let before = region_rc(&heap, rid);

    let claimed = release_displaced_denial_payload(&mut heap, &handle);

    assert!(!claimed, "an unrecorded park is not a denial");
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
    let handle = parked_fiber(SIG_YIELD, p, Some(p));

    assert!(release_displaced_denial_payload(&mut heap, &handle));
    let after_first = region_rc(&heap, rid);

    let claimed = release_displaced_denial_payload(&mut heap, &handle);

    assert!(
        !claimed,
        "the record is gone — the second install claims nothing"
    );
    assert_eq!(
        region_rc(&heap, rid),
        after_first,
        "one reference is owed per park, however many installs displace it",
    );
}

/// A fiber denied `:io` parks under `SIG_IO`, the very bit an io request's park
/// carries, so there the two readings name the same park. The record answers, and
/// `prim_fiber_resume` skips the io arm on this `true` — running both frees the
/// payload under the mediator that is still reading it.
#[test]
fn an_io_denial_is_claimed_by_the_record_that_names_it() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = parked_fiber(SIG_IO, p, Some(p));
    let before = region_rc(&heap, rid);

    let claimed = release_displaced_denial_payload(&mut heap, &handle);

    assert!(claimed, "the record tells the denial from the io request");
    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "the collision owes ONE reference, not one per reading",
    );
}

// -- release_parked_signal: the io arm, gated on SIG_IO alone --

/// The io arm keeps its shared-region skip: a `Fresh` io op builds its completion
/// buffer in the request's own region and hands it back as the resume value, so
/// that region is still live and the caller's release answers for it.
#[test]
fn an_io_park_whose_resume_value_shares_its_region_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = payload(&mut heap);
    let completion = heap.alloc_in_region(cons(), rid);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_IO, request)), completion);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the completion buffer built in the request's region keeps it live",
    );
}

/// …and releases when the resume value comes from elsewhere, which is the io
/// request's own orphaned escape retain.
#[test]
fn an_io_park_whose_resume_value_is_foreign_is_released() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = payload(&mut heap);
    let completion = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_IO, request)), completion);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a submitted io request is dead at the resume — its region owes one decref",
    );
}

/// The counter-factual for the io gate: a `yield`/`emit` payload is body-owned.
/// The resumed body releases the reference it held across the suspend, so a
/// decref here would free the value under every holder that outlives the fiber.
#[test]
fn a_non_io_park_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let resume_value = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_YIELD, p)), resume_value);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the io arm answers for io requests alone",
    );
}
