//! The ledger's transitions: each park write, each consume seam, and the
//! debug net for a park shape wired without a consume.

use super::*;

/// A heap-shaped payload stand-in. The ledger never dereferences what it
/// names — identity is bit-wise — so any distinct non-nil values serve.
fn payload() -> Value {
    Value::int(1)
}

fn other() -> Value {
    Value::int(2)
}

#[test]
fn a_primitive_park_funds_exactly_one_resume() {
    let mut d = Delivery::new();
    d.park_primitive();
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
    d.park_denial(payload());
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
    d.park_denial(payload());
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
    d.park_denial(payload());
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
    d.park_denial(payload());
    d.record_mint(payload());
    d.discharge();
    assert!(d.bodyless().is_none());
    assert!(!d.mint_names(payload()));
    assert!(
        !d.take_resume_funding(),
        "a discharged park is over — a later resume of the killed fiber must mint nothing",
    );
}

/// The net for a new park shape wired without a consume seam: parking over an
/// unconsumed park means some route ended the previous park without passing
/// `take_resume_funding`, `install_abort`, or `discharge` — a leak of one
/// region per cycle in release builds, a panic here.
#[test]
#[should_panic(expected = "unconsumed")]
#[cfg(debug_assertions)]
fn a_second_park_over_an_unconsumed_one_panics() {
    let mut d = Delivery::new();
    d.park_primitive();
    d.park_primitive();
}

#[test]
#[should_panic(expected = "unconsumed")]
#[cfg(debug_assertions)]
fn a_denial_park_over_an_unconsumed_one_panics() {
    let mut d = Delivery::new();
    d.park_primitive();
    d.park_denial(payload());
}
