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

/// An `IoRequest` on a region of its own — the payload a yielding io op parks,
/// and the one thing the io arm answers for. A `Sleep` because it is the portless
/// op: nothing else in the request has to be built for the type to be right.
fn io_request(
    heap: &mut crate::value::fiberheap::FiberHeap,
) -> (Value, crate::hir::region::RuntimeRegion) {
    use crate::value::heap::ExternalObject;
    let obj = HeapObject::External {
        obj: ExternalObject {
            type_name: "io-request",
            data: Rc::new(crate::io::request::IoRequest {
                op: crate::io::request::IoOp::Sleep {
                    duration: std::time::Duration::from_secs(1),
                },
                port: Value::NIL,
                timeout: None,
            }),
        },
        traits: Value::NIL,
    };
    alloc_in_fresh_region(heap, obj)
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
        if let Some(payload) = record {
            f.delivery.park_denial(payload);
        }
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

    release_displaced_denial_payload(&mut heap, &handle);

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

    release_displaced_denial_payload(&mut heap, &handle);

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

    release_displaced_denial_payload(&mut heap, &handle);

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

    release_displaced_denial_payload(&mut heap, &handle);
    let after_first = region_rc(&heap, rid);

    release_displaced_denial_payload(&mut heap, &handle);

    assert_eq!(
        region_rc(&heap, rid),
        after_first,
        "one reference is owed per park, however many installs displace it",
    );
}

/// A fiber denied `:io` parks under `SIG_IO`, the very bit a yielding io op's
/// request park carries, so the bits alone would hand one park to both readings.
/// The record answers and the io arm stands down, because the payload is a
/// struct rather than an `IoRequest` — one reference is owed, not one per
/// reading.
///
/// The trap: the record lives on the fiber that was DENIED, and an install can
/// reach a fiber that merely relays the park (the outer fiber of a `protect`ed
/// denial), where there is no record to ask. So the exclusion cannot be an
/// ordering between the two calls — it has to be each reading's own, which is
/// what the type test buys. Ordering instead frees the struct under the mediator
/// on exactly that route.
#[test]
fn an_io_denial_answers_to_the_record_and_not_to_the_io_arm() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let handle = parked_fiber(SIG_IO, p, Some(p));
    let before = region_rc(&heap, rid);

    release_displaced_io_request(&mut heap, Some((SIG_IO, p)));
    let after_io_arm = region_rc(&heap, rid);
    release_displaced_denial_payload(&mut heap, &handle);

    assert_eq!(
        after_io_arm, before,
        "a denial payload is not an IoRequest — the io arm has no claim on it",
    );
    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "the collision owes ONE reference, and it is the record's",
    );
}

// -- release_displaced_io_request: the io arm, on a park's own IoRequest --

/// An io park's request is the runtime's value, so whatever ends the park owes
/// the reference the allocation left. The injection `fiber/abort` and
/// `fiber/refuse` share reaches this arm with no resume value at all.
#[test]
fn a_displaced_io_request_is_released() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let before = region_rc(&heap, rid);

    release_displaced_io_request(&mut heap, Some((SIG_IO, request)));

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "the install that displaces an io park owes its request one decref",
    );
}

/// The counter-factual for the io gate: a `yield`/`emit` payload is body-owned.
/// The resumed body releases the reference it held across the suspend, so a
/// decref here would free the value under every holder that outlives the fiber.
#[test]
fn a_non_io_park_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    let before = region_rc(&heap, rid);

    release_displaced_io_request(&mut heap, Some((SIG_YIELD, p)));

    assert_eq!(
        region_rc(&heap, rid),
        before,
        "the io arm answers for io requests alone",
    );
}

/// The trap: the bits must be read BEFORE the payload is dereferenced. A park
/// this arm has no claim on is one whose region another holder may already have
/// released, and `region_of` answers such a value with a stale-deref panic rather
/// than with `None`. Reading the value first turns every non-io park into that
/// panic once its region is gone.
#[test]
fn a_non_io_park_is_answered_without_dereferencing_its_payload() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (p, rid) = payload(&mut heap);
    crate::value::arena::decref_region(&mut heap, Some(rid));

    release_displaced_io_request(&mut heap, Some((SIG_YIELD, p)));
}

// -- release_resumed_io_request: the same arm, plus the delivery's skip --

/// The resume adds a shared-region skip: a `Fresh` io op builds its completion
/// buffer in the request's own region and hands it back as the resume value, so
/// that region is still live and the caller's release answers for it.
#[test]
fn an_io_park_whose_resume_value_shares_its_region_releases_nothing() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let completion = heap.alloc_in_region(cons(), rid);
    let before = region_rc(&heap, rid);

    release_resumed_io_request(&mut heap, Some((SIG_IO, request)), completion);

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
    let (request, rid) = io_request(&mut heap);
    let completion = foreign(&mut heap);
    let before = region_rc(&heap, rid);

    release_resumed_io_request(&mut heap, Some((SIG_IO, request)), completion);

    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "a submitted io request is dead at the resume — its region owes one decref",
    );
}

/// The skip belongs to the DELIVERY, not to the io park: an injected error that
/// happens to live in the request's region is not a resume value, so the
/// injection's release stands. Sharing a region is what the resume reads as
/// "still live"; here it means nothing.
#[test]
fn a_displaced_io_request_takes_no_skip_from_a_payload_sharing_its_region() {
    let mut heap = crate::value::fiberheap::FiberHeap::new();
    let (request, rid) = io_request(&mut heap);
    let error_value = heap.alloc_in_region(cons(), rid);
    let before = region_rc(&heap, rid);

    release_resumed_io_request(&mut heap, Some((SIG_IO, request)), error_value);
    let after_resume_reading = region_rc(&heap, rid);
    release_displaced_io_request(&mut heap, Some((SIG_IO, request)));

    assert_eq!(
        after_resume_reading, before,
        "the resume reading skips on a shared region — which is why it must not travel",
    );
    assert_eq!(
        region_rc(&heap, rid),
        before - 1,
        "an injected error sharing the request's region still owes the release",
    );
}
