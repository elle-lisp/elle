//! The resume-path gate: which parks owe their payload a release, and which
//! must be left to the resumed body.

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::heap::{HeapObject, Pair};
use crate::value::{SIG_IO, SIG_YIELD};

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

/// A capability denial's park has no body reference, so the resume releases the
/// one the payload is left with (docs/impl/region/owner.md § "A park with no body
/// reference owes one release at the resume"). The blocked bits are the park's,
/// and they are NOT `SIG_IO` here — the io gate alone would let a denial of any
/// other capability through, stranding the payload's region once per mediation.
#[test]
fn a_recorded_denial_park_is_released_at_the_resume() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let resume_value = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_YIELD, p)), Some(p), resume_value);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a mediated denial's payload region owes exactly one decref at the resume",
    );
}

/// The record is matched against the live parked signal, so an install that
/// displaced the denial payload without resuming — `fiber/abort`'s injection, a
/// hard kill's terminal error — leaves no release for the resume to run.
#[test]
fn a_record_that_no_longer_names_the_parked_signal_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (parked, rid) = payload(&mut heap);
    let (stale, _) = payload(&mut heap);
    let resume_value = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_parked_signal(
        &mut heap,
        Some((SIG_YIELD, parked)),
        Some(stale),
        resume_value,
    );

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the decref is owed by the park the record names, not by whatever \
         occupies the signal slot later",
    );
}

/// The counter-factual for both: a `yield`/`emit` payload is body-owned. The
/// resumed body releases the reference it held across the suspend, so a decref
/// here would free the value under every holder that outlives the fiber.
#[test]
fn an_unrecorded_yield_park_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let resume_value = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_YIELD, p)), None, resume_value);

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "a body-owned park owes the resume path nothing",
    );
}

/// The io arm keeps its shared-region skip: a `Fresh` io op builds its completion
/// buffer in the request's own region and hands it back as the resume value, so
/// that region is still live and the caller's release answers for it.
#[test]
fn an_io_park_whose_resume_value_shares_its_region_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = payload(&mut heap);
    let completion = heap.alloc_in_region(cons(), rid);
    let before = region_rc(&heap, rid);

    release_parked_signal(&mut heap, Some((SIG_IO, request)), None, completion);

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

    release_parked_signal(&mut heap, Some((SIG_IO, request)), None, completion);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a submitted io request is dead at the resume — its region owes one decref",
    );
}
