// audited: 2026-10-05
//! What a call boundary lets out: a squelch's predicate, and the signals that break a silent function's proof.
//!
//! docs/signals/inference.md

use super::super::*;

// ── squelched_bits: what a squelch/attune boundary enforces ─────────
//
// The interpreter and both JIT call paths share this predicate, so its
// exemption classes are the tier-independent statement of the rule.
// `tests/lang/squelch-fuel.lisp` pins the pause exemption end to end.

#[test]
fn squelched_bits_names_the_bits_the_mask_covers() {
    assert_eq!(squelched_bits(SIG_YIELD, SIG_YIELD), SIG_YIELD);
    assert_eq!(
        squelched_bits(SIG_YIELD.union(SIG_IO), SIG_IO),
        SIG_IO,
        "only the covered bit violates the boundary"
    );
}

#[test]
fn squelched_bits_is_empty_when_the_mask_misses() {
    assert!(squelched_bits(SIG_YIELD, SIG_IO).is_empty());
    assert!(squelched_bits(SIG_YIELD, SignalBits::EMPTY).is_empty());
    assert!(squelched_bits(SignalBits::EMPTY, SignalBits::ALL).is_empty());
}

#[test]
fn squelched_bits_exempts_error_and_halt_whole() {
    // Both are the escapes every boundary lets out, so a compound carrying
    // one passes entire — the squelched companion bit does not violate.
    assert!(squelched_bits(SIG_ERROR, SignalBits::ALL).is_empty());
    assert!(squelched_bits(SIG_HALT, SignalBits::ALL).is_empty());
    assert!(squelched_bits(SIG_ERROR.union(SIG_YIELD), SIG_YIELD).is_empty());
    assert!(squelched_bits(SIG_HALT.union(SIG_YIELD), SIG_YIELD).is_empty());
}

#[test]
fn squelched_bits_exempts_switch_by_exact_match_only() {
    assert!(squelched_bits(SIG_SWITCH, SignalBits::ALL).is_empty());
    // A user signal riding alongside the trampoline bit stays enforceable.
    assert_eq!(
        squelched_bits(SIG_SWITCH.union(SIG_YIELD), SIG_YIELD),
        SIG_YIELD
    );
}

#[test]
fn squelched_bits_exempts_the_pause_bits() {
    // The VM injects a pause at its own charge sites; the metering parent
    // owns it, so a boundary that names :fuel has nothing to enforce.
    assert!(squelched_bits(SIG_FUEL, SIG_FUEL).is_empty());
    assert!(squelched_bits(SIG_PAUSE, SignalBits::ALL).is_empty());
}

#[test]
fn squelched_bits_passes_a_signal_that_carries_the_pause_whole() {
    // A dynamic breakpoint pauses with :debug|:fuel. The VM raised it, not
    // the code under the boundary, so a mask that names :debug must not
    // convert it. Subtracting :fuel alone would leave :debug to violate.
    assert!(squelched_bits(SIG_FUEL.union(SIG_DEBUG), SIG_DEBUG).is_empty());
    assert!(squelched_bits(SIG_FUEL.union(SIG_YIELD), SIG_FUEL.union(SIG_YIELD)).is_empty());
    assert!(squelched_bits(SIG_FUEL.union(SIG_DEBUG), SignalBits::ALL).is_empty());
    // Plain :debug is the code's own emit, and stays enforceable.
    assert_eq!(squelched_bits(SIG_DEBUG, SIG_DEBUG), SIG_DEBUG);
}

// ── breaks_proof: what leaves a silent function ─────────────────────
//
// A silent function is a proof that nothing stops the fiber, so every
// signal that leaves one breaks it, except a halt, which is a resource
// fault, and a signal already carrying :terminal, which is an inner broken
// proof on its way out. src/vm/call/proof_tests.rs pins the report end to
// end.

#[test]
fn nothing_leaving_breaks_no_proof() {
    assert!(!breaks_proof(SignalBits::EMPTY));
}

#[test]
fn any_raise_breaks_the_proof_error_included() {
    // Counter-factual: the silent boundary was a squelch over every signal,
    // which exempts :error, the half of the proof every caller relies on.
    assert!(breaks_proof(SIG_ERROR));
    assert!(breaks_proof(SIG_YIELD));
    assert!(breaks_proof(SIG_IO.union(SIG_ERROR)));
    assert!(breaks_proof(SignalBits::from_bit(40)), "a user signal");
}

#[test]
fn a_pause_breaks_the_proof() {
    // The interpreter charges no fuel inside a silent function, so a pause
    // that leaves one came from code the proof says cannot stop.
    assert!(breaks_proof(SIG_FUEL));
    assert!(breaks_proof(SIG_FUEL.union(SIG_DEBUG)));
}

#[test]
fn a_halt_and_a_terminal_signal_pass() {
    assert!(
        !breaks_proof(SIG_HALT),
        "a resource fault is not a broken proof"
    );
    assert!(!breaks_proof(SIG_HALT.union(SIG_ERROR)));
    assert!(
        !breaks_proof(SIG_ERROR.union(SIG_TERMINAL)),
        "an inner broken proof passes whole"
    );
}
