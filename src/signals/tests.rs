//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_signal_combine_silent() {
    assert_eq!(Signal::silent().combine(Signal::silent()), Signal::silent());
}

#[test]
fn test_signal_combine_yields() {
    assert_eq!(Signal::silent().combine(Signal::yields()), Signal::yields());
    assert_eq!(Signal::yields().combine(Signal::silent()), Signal::yields());
    assert_eq!(Signal::yields().combine(Signal::yields()), Signal::yields());
}

#[test]
fn test_signal_combine_polymorphic() {
    assert_eq!(
        Signal::silent().combine(Signal::polymorphic(0)),
        Signal::polymorphic(0)
    );
    assert_eq!(
        Signal::polymorphic(1).combine(Signal::silent()),
        Signal::polymorphic(1)
    );
    // Polymorphic + Yields = both
    let combined = Signal::polymorphic(0).combine(Signal::yields());
    assert!(combined.may_yield());
    assert!(combined.is_polymorphic());
}

#[test]
fn test_signal_combine_polymorphic_multiple() {
    let combined = Signal::polymorphic(0).combine(Signal::polymorphic(1));
    assert_eq!(
        combined,
        Signal {
            bits: SignalBits::new(0),
            propagates: 0b11,
        }
    );

    let combined2 = Signal::polymorphic(0).combine(Signal::polymorphic(0));
    assert_eq!(combined2, Signal::polymorphic(0));
}

#[test]
fn test_signal_combine_all() {
    assert_eq!(
        Signal::combine_all([Signal::silent(), Signal::silent(), Signal::silent()]),
        Signal::silent()
    );
    assert_eq!(
        Signal::combine_all([Signal::silent(), Signal::yields(), Signal::silent()]),
        Signal::yields()
    );
}

#[test]
fn test_may_suspend() {
    assert!(!Signal::silent().may_suspend());
    assert!(Signal::errors().may_suspend()); // error is a fiber transfer
    assert!(Signal::yields().may_suspend());
    assert!(Signal::polymorphic(0).may_suspend());
    assert!(Signal {
        bits: SIG_DEBUG,
        propagates: 0,
    }
    .may_suspend());
}

#[test]
fn test_may_yield() {
    assert!(!Signal::silent().may_yield());
    assert!(Signal::yields().may_yield());
    assert!(!Signal::errors().may_yield());
}

#[test]
fn test_may_error() {
    assert!(!Signal::silent().may_error());
    assert!(Signal::errors().may_error());
    assert!(!Signal::yields().may_error());
    assert!(Signal::yields_errors().may_error());

    // Combining errors
    let combined = Signal::silent().combine(Signal::errors());
    assert!(combined.may_error());
    assert!(combined.may_suspend()); // error is a suspension
}

#[test]
fn test_may_ffi() {
    assert!(!Signal::silent().may_ffi());
    assert!(Signal::ffi().may_ffi());
    assert!(Signal::ffi_errors().may_ffi());
}

#[test]
fn test_ffi_errors() {
    let e = Signal::ffi_errors();
    assert!(e.may_ffi());
    assert!(e.may_error());
    assert!(!e.may_yield());
    assert!(e.may_suspend()); // FFI+error = has signal bits = may suspend
    assert!(!e.is_polymorphic());
}

#[test]
fn test_is_polymorphic() {
    assert!(!Signal::silent().is_polymorphic());
    assert!(Signal::polymorphic(0).is_polymorphic());
}

#[test]
fn test_signal_display() {
    assert_eq!(format!("{}", Signal::silent()), "silent");
    assert_eq!(format!("{}", Signal::yields()), "yields");
    assert_eq!(format!("{}", Signal::errors()), "silent+errors");
    assert_eq!(format!("{}", Signal::yields_errors()), "yields+errors");
    assert_eq!(format!("{}", Signal::polymorphic(0)), "polymorphic(0)");
    assert_eq!(
        format!("{}", Signal::polymorphic_errors(0)),
        "polymorphic(0)+errors"
    );
    assert_eq!(format!("{}", Signal::ffi()), "silent+ffi");
    assert_eq!(format!("{}", Signal::ffi_errors()), "silent+errors+ffi");
}

#[test]
fn test_propagated_params() {
    let e = Signal {
        bits: SignalBits::new(0),
        propagates: 0b101, // params 0 and 2
    };
    let params: Vec<_> = e.propagated_params().collect();
    assert_eq!(params, vec![0, 2]);
}

#[test]
fn test_signal_is_copy() {
    let e = Signal::yields();
    let e2 = e; // Copy
    assert_eq!(e, e2);
}

#[test]
fn test_constants() {
    assert_eq!(Signal::SILENT, Signal::silent());
    assert_eq!(Signal::YIELDS, Signal::yields());
}

#[test]
fn test_sig_exec_bit_is_distinct() {
    // SIG_EXEC must be a unique bit (bit 11).
    assert_eq!(SIG_EXEC, SignalBits::from_bit(11));
    // Must not overlap with any other defined signal bits.
    assert!(!SIG_EXEC.intersects(SIG_IO));
    assert!(!SIG_EXEC.intersects(SIG_YIELD));
    assert!(!SIG_EXEC.intersects(SIG_TERMINAL));
}

#[test]
fn test_exec_keyword_registered() {
    use crate::signals::registry::global_registry;
    // The :exec keyword must be registered and map to SIG_EXEC.
    let reg = global_registry().lock().unwrap();
    let bit_pos = reg.lookup("exec").expect(":exec must be registered");
    // lookup returns the bit position (11), not the bitmask; verify both.
    assert_eq!(bit_pos, 11);
    assert_eq!(SignalBits::from_bit(bit_pos), SIG_EXEC);
}

#[test]
fn test_fuel_bit_is_distinct() {
    // SIG_FUEL must be a unique bit (bit 12).
    assert_eq!(SIG_FUEL, SignalBits::from_bit(12));
    // Must not overlap with any other defined signal bits.
    assert!(!SIG_FUEL.intersects(SIG_EXEC));
    assert!(!SIG_FUEL.intersects(SIG_IO));
    assert!(!SIG_FUEL.intersects(SIG_TERMINAL));
}

#[test]
fn test_fuel_keyword_registered() {
    use crate::signals::registry::global_registry;
    let reg = global_registry().lock().unwrap();
    let bit_pos = reg.lookup("fuel").expect(":fuel must be registered");
    assert_eq!(bit_pos, 12);
    assert_eq!(SignalBits::from_bit(bit_pos), SIG_FUEL);
}

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
