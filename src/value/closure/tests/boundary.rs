// audited: 2026-11-23
//! The parts a call reads from a closure before it runs build the boundary the closure itself would.
//!
//! docs/signals/jit.md

use super::*;
use crate::signals::Signal;
use crate::value::fiber::SIG_YIELD;

/// A closure over a fresh code object on `heap` with `signal`, squelched by
/// `mask`, and no environment.
fn closure_with(heap: &mut FiberHeap, signal: Signal, mask: SignalBits) -> Closure {
    let template = CodeBuilder::new(vec![0], Arity::Exact(0), Vec::new())
        .signal(signal)
        .build(heap);
    Closure::new(
        template,
        crate::value::region_slice::RegionSlice::empty(),
        mask,
    )
}

/// The payload a boundary's proof names, or `None` where it proves nothing.
fn proof_payload(boundary: &Boundary) -> Option<*const CodePayload> {
    boundary
        .proof
        .as_ref()
        .map(|t| t.payload() as *const CodePayload)
}

/// A compiled call reads these parts before the call and builds the boundary
/// only when a signal comes back (docs/signals/jit.md). The boundary must be
/// the one `Boundary::of` builds: the closure's squelch mask, and its function
/// as the proof exactly when the function is silent.
///
/// The counter-factual is parts that settle the proof without the signal,
/// for example by always naming the function: a squelched yielding closure
/// then carries a proof, and its first yield raises `:broken-proof`.
#[test]
fn parts_read_before_a_call_build_the_closures_boundary() {
    let mut heap = FiberHeap::new();
    let silent = closure_with(&mut heap, Signal::silent(), SignalBits::EMPTY);
    let yields = closure_with(&mut heap, Signal::yields(), SIG_YIELD);

    let built = BoundaryParts::of(&silent).boundary();
    assert_eq!(built.squelch, SignalBits::EMPTY);
    assert_eq!(
        proof_payload(&built),
        Some(silent.template.payload() as *const CodePayload),
        "a silent function's boundary names it as the proof"
    );

    let built = BoundaryParts::of(&yields).boundary();
    assert_eq!(built.squelch, SIG_YIELD, "the squelch mask comes across");
    assert_eq!(
        proof_payload(&built),
        None,
        "a function that may yield proves nothing"
    );

    for closure in [&silent, &yields] {
        let (parts, direct) = (BoundaryParts::of(closure).boundary(), Boundary::of(closure));
        assert_eq!(parts.squelch, direct.squelch);
        assert_eq!(proof_payload(&parts), proof_payload(&direct));
    }
}
