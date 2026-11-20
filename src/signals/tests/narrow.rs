// audited: 2026-11-16
//! What `Signal::squelch` leaves of a signal: the blocked bits go, `:error` comes in, and the bits every boundary passes stay.
//!
//! docs/signals/squelch.md

use super::super::*;

#[test]
fn test_squelch_noop_when_mask_irrelevant() {
    // Squelching :yield on a silent function changes nothing.
    let sig = Signal::errors();
    let result = sig.squelch(SIG_YIELD);
    assert_eq!(result, sig);
}

#[test]
fn test_squelch_clears_bits_adds_error() {
    // Squelching :yield on a yields function clears yield, adds error.
    let sig = Signal::yields();
    let result = sig.squelch(SIG_YIELD);
    assert!(!result.may_yield());
    assert!(result.may_error());
    assert!(result.may_suspend()); // squelch adds SIG_ERROR = suspension
}

#[test]
fn test_squelch_preserves_propagates() {
    // Squelch preserves the propagates mask.
    let sig = Signal {
        bits: SIG_YIELD.union(SIG_ERROR),
        propagates: 0b101,
    };
    let result = sig.squelch(SIG_YIELD);
    assert_eq!(result.propagates, 0b101);
    assert!(!result.bits.intersects(SIG_YIELD));
    assert!(result.bits.intersects(SIG_ERROR));
}

#[test]
fn test_squelch_multiple_bits() {
    // Squelch a set of signals.
    let sig = Signal {
        bits: SIG_YIELD.union(SIG_IO).union(SIG_ERROR),
        propagates: 0,
    };
    let mask = SIG_YIELD.union(SIG_IO);
    let result = sig.squelch(mask);
    assert!(!result.bits.intersects(SIG_YIELD));
    assert!(!result.bits.intersects(SIG_IO));
    assert!(result.bits.intersects(SIG_ERROR));
}

#[test]
fn test_squelch_yields_errors_becomes_silent_errors() {
    // Squelching :yield on yields+errors → errors only.
    let sig = Signal::yields_errors();
    let result = sig.squelch(SIG_YIELD);
    assert!(!result.may_yield());
    assert!(result.may_error());
    assert!(result.may_suspend()); // error remains = still suspending
}

#[test]
fn test_squelch_all_bits_leaves_error() {
    // Squelching everything still leaves error (from squelch itself).
    let sig = Signal {
        bits: SIG_YIELD.union(SIG_IO),
        propagates: 0,
    };
    let mask = SIG_YIELD.union(SIG_IO);
    let result = sig.squelch(mask);
    assert_eq!(result.bits, SIG_ERROR);
}

/// `:error` and `:halt` cross every boundary at run time, so a mask that names
/// either removes nothing. Counter-factual: the mask removed `:halt`, so a
/// binding of `(squelch :halt f)` over an `f` that halts claimed it could not
/// halt, and a ceiling without `:halt` accepted a call to it.
#[test]
fn squelch_keeps_the_bits_that_cross_every_boundary() {
    let sig = Signal::ambient_halts();
    assert_eq!(sig.squelch(SIG_HALT), sig);
    assert_eq!(sig.squelch(SIG_ERROR), sig);
    assert_eq!(
        sig.squelch(SIG_HALT.union(SIG_AMBIENT)).bits,
        SIG_HALT.union(SIG_ERROR),
        "the blocked :ambient goes and the :halt stays"
    );
}

/// `attune` blocks every bit outside its spec. A closure the compiler knows
/// nothing about may raise every bit, and narrows to the spec, with the
/// `:error` the boundary may raise and the `:halt` that crosses it.
#[test]
fn attune_narrows_an_unknown_closure_to_its_spec() {
    let blocked = CAP_MASK.subtract(SIG_YIELD);
    assert_eq!(
        Signal::unknown().squelch(blocked).bits,
        SIG_YIELD.union(SIG_ERROR).union(SIG_HALT)
    );
}
