//! The ledger's transitions: each park write, each consume seam, and the
//! debug net for a park shape wired without a consume.

use super::*;
use crate::value::{SIG_IO, SIG_YIELD};

/// A payload on a region of its own. The ledger never dereferences what it
/// names — identity is bit-wise — but it does gate the park record on the value
/// living in a region at all, so a park stand-in has to be a heap value. The
/// heap is leaked so the pair stays resident for the test.
fn payload() -> Value {
    PAIRS.with(|p| p.0)
}

fn other() -> Value {
    PAIRS.with(|p| p.1)
}

thread_local! {
    /// Two distinct heap pairs, minted once per test thread so every call
    /// returns the same representation — the gates below are all bit-wise
    /// identity, so a fresh pair per call would name a different value each time.
    static PAIRS: (Value, Value) = (heap_pair(), heap_pair());
}

fn heap_pair() -> Value {
    use crate::value::heap::{HeapObject, Pair};
    let heap = crate::value::arena::leaked_test_heap();
    let (v, _) = crate::value::arena::alloc_in_fresh_region(
        unsafe { &mut *heap },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    v
}

/// The immediate face of the same question: a park whose payload lives in no
/// region took no escape retain, so there is nothing for a seam to consume.
fn immediate() -> Value {
    Value::int(1)
}

#[test]
fn a_primitive_park_funds_exactly_one_resume() {
    let mut d = Delivery::new();
    d.park_primitive(SIG_IO, payload());
    assert!(
        d.take_resume_funding(),
        "the park's resume value owes a mint"
    );
    assert!(
        !d.take_resume_funding(),
        "a second take finds nothing — a later park of a different shape starts from false",
    );
}

#[test]
fn a_denial_park_records_both_facts() {
    let mut d = Delivery::new();
    d.park_denial(SIG_IO, payload());
    assert_eq!(
        d.bodyless().map(|p| p.bit_identical(payload())),
        Some(true),
        "the denial payload is recorded for the resume's decref",
    );
    assert!(d.take_resume_funding(), "a denial is a primitive park too");
}

#[test]
fn an_abort_install_displaces_the_park_and_records_the_mint() {
    let mut d = Delivery::new();
    d.park_denial(SIG_IO, payload());
    d.install_abort(other());
    assert!(
        d.bodyless().is_none(),
        "the displaced denial record left with its payload"
    );
    assert!(
        d.mint_names(other()),
        "the injection's mint travels with the new payload"
    );
    assert!(
        !d.take_resume_funding(),
        "an abort delivers no resume value, so the park's mint obligation goes with it",
    );
}

#[test]
fn the_mint_gate_is_representation_identity() {
    let mut d = Delivery::new();
    d.record_mint(payload());
    assert!(d.mint_names(payload()));
    assert!(
        !d.mint_names(other()),
        "a record naming a value no longer in the slot withholds nothing",
    );
}

#[test]
fn a_displace_clears_the_payload_records_but_keeps_the_resume_funding() {
    let mut d = Delivery::new();
    d.park_denial(SIG_IO, payload());
    d.record_mint(payload());
    d.displace();
    assert!(d.bodyless().is_none());
    assert!(!d.mint_names(payload()));
    // The trampoline's FiberResume short-circuit installs the resume value and
    // displaces, but the delivery funnel that consumes the funding runs later —
    // clearing it here would skip the ResumeDelivery mint the replayed frame's
    // result release consumes.
    assert!(
        d.take_resume_funding(),
        "the funnel still owes the replayed frame its resume-value mint",
    );
}

#[test]
fn a_discharge_leaves_no_funding() {
    let mut d = Delivery::new();
    d.park_denial(SIG_IO, payload());
    d.record_mint(payload());
    d.discharge();
    assert!(d.bodyless().is_none());
    assert!(!d.mint_names(payload()));
    assert!(
        !d.take_resume_funding(),
        "a discharged park is over — a later resume of the killed fiber must mint nothing",
    );
}

// -- the undelivered park: the record a boundary reads --

/// The record names the payload the park's escape retain was taken on, so the
/// boundary that ends the park releases the right region. Both bits and payload
/// travel: the io arm reads the bits before it dereferences anything.
#[test]
fn a_park_records_the_payload_its_retain_was_taken_on() {
    let mut d = Delivery::new();
    d.park_primitive(SIG_IO, payload());
    let taken = d.take_undelivered().expect("the park is recorded");
    assert_eq!(taken.0, SIG_IO, "the park's bits travel with its payload");
    assert!(taken.1.bit_identical(payload()));
}

/// The crossing consumes the delivery: the resumer read the payload out and its
/// own release of the resume result takes the retain. Counter-factual: a record
/// surviving the crossing lets a later boundary release a reference the resumer
/// already consumed, freeing the payload under the reader that still holds it.
#[test]
fn the_crossing_leaves_no_park_for_a_boundary_to_claim() {
    let mut d = Delivery::new();
    d.park_primitive(SIG_YIELD, payload());
    d.take_resume_funding();
    assert!(
        d.take_undelivered().is_none(),
        "a park a resumer read is delivered — the retain has its consumer",
    );
}

/// An `Emit` node's park records the retain and no resume funding: the compiler
/// balances that continuation itself. Counter-factual: routing it through
/// `park_primitive` would mint a `ResumeDelivery` reference nothing consumes.
#[test]
fn an_emit_park_records_the_retain_and_owes_no_resume_mint() {
    let mut d = Delivery::new();
    d.park_emit(SIG_YIELD, payload());
    assert!(
        d.take_undelivered().is_some(),
        "the emit's escape retain is a delivery a boundary may have to release",
    );
    assert!(
        !d.take_resume_funding(),
        "an `Emit` node's continuation is funded by its own decref_point",
    );
}

/// Taking the record IS the receipt: a second boundary over the same fiber
/// finds nothing. Counter-factual: a non-consuming read releases the same
/// reference once per boundary.
#[test]
fn the_park_record_is_taken_so_a_second_boundary_claims_nothing() {
    let mut d = Delivery::new();
    d.park_emit(SIG_YIELD, payload());
    d.take_undelivered();
    assert!(d.take_undelivered().is_none());
}

/// The net for a new park shape wired without a consume seam: parking over an
/// unconsumed park means some route ended the previous park without passing
/// `take_resume_funding`, `install_abort`, `displace`, or `discharge` — a leak
/// of one region per cycle in release builds, a panic here.
#[test]
#[should_panic(expected = "unconsumed")]
#[cfg(debug_assertions)]
fn a_second_park_over_an_unconsumed_one_panics() {
    let mut d = Delivery::new();
    d.park_primitive(SIG_IO, payload());
    d.park_primitive(SIG_IO, payload());
}

#[test]
#[should_panic(expected = "unconsumed")]
#[cfg(debug_assertions)]
fn a_denial_park_over_an_unconsumed_one_panics() {
    let mut d = Delivery::new();
    d.park_primitive(SIG_IO, payload());
    d.park_denial(SIG_IO, payload());
}

/// The net does NOT cover the park record, and must not: an `Emit` park owes no
/// resume mint, so a route that ends one without clearing the record leaves
/// nothing owed — the record is payload-named and its consumer compares it
/// bit-wise, exactly as the denial record's does. Asserting on it instead would
/// panic on a park some host abandoned, where nothing is wrong.
#[test]
#[cfg(debug_assertions)]
fn a_park_over_a_stale_park_record_is_fine() {
    let mut d = Delivery::new();
    d.park_emit(SIG_YIELD, payload());
    d.park_emit(SIG_YIELD, other());
    assert_eq!(
        d.take_undelivered().map(|(_, v)| v.bit_identical(other())),
        Some(true),
        "the later park's record replaces the stale one",
    );
}

/// An IMMEDIATE payload records no park, because the escape retain was a no-op
/// on a value living in no region. Counter-factual: recording it puts the net
/// above in the way of a park that owes nothing — every `(emit :yield 1)` would
/// have to reach a consume seam that has nothing to consume.
#[test]
fn an_immediate_park_records_nothing() {
    let mut d = Delivery::new();
    d.park_emit(SIG_YIELD, immediate());
    assert!(
        d.take_undelivered().is_none(),
        "a payload in no region took no retain, so no seam owes it a release",
    );
}
