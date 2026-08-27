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

// ── Named constructors ───────────────────────────────────────────────
//
// Each pins the bits its name promises. The set is what the analyzer reasons
// about, so a constructor whose name and bits disagree mis-declares every
// primitive that uses it.

#[test]
fn io_yields_errors_names_io_and_error_only() {
    let sig = Signal::io_yields_errors();
    assert!(sig.bits.intersects(SIG_IO), "must name :io");
    assert!(sig.bits.intersects(SIG_ERROR), "must name :error");
    assert!(
        !sig.bits.intersects(SIG_YIELD),
        "the request suspends its fiber, but suspension follows from raising \
         any signal — :yield is the keyword a generator's mask names"
    );
    assert_eq!(sig.propagates, 0);
}

#[test]
fn query_errors_names_query_and_error_only() {
    let sig = Signal::query_errors();
    assert_eq!(sig.bits, SIG_QUERY.union(SIG_ERROR));
    assert_eq!(sig.propagates, 0);
}

#[test]
fn of_carries_the_bits_through_and_propagates_nothing() {
    let bits = SIG_ERROR.union(SIG_ABORT);
    assert_eq!(Signal::of(bits).bits, bits);
    assert_eq!(Signal::of(bits).propagates, 0);
}

#[test]
fn subprocess_names_the_dispatch_bit_and_the_capability_bit() {
    let sig = Signal::subprocess();
    assert!(
        sig.bits.intersects(SIG_IO),
        ":io routes the request through the scheduler"
    );
    assert!(
        sig.bits.intersects(SIG_EXEC),
        ":exec is what a fiber mask denies to forbid spawning"
    );
    assert!(sig.bits.intersects(SIG_ERROR));
    assert!(!sig.bits.intersects(SIG_YIELD), "and it claims no :yield");
}

// ── squelched_bits: what a squelch/attune boundary enforces ─────────
//
// The interpreter and both JIT call paths share this predicate, so its
// exemption classes are the tier-independent statement of the rule.
// `tests/elle/squelch-fuel.lisp` pins the pause exemption end to end.

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
fn squelched_bits_subtracts_the_pause_rather_than_exempting_the_signal() {
    // A pause riding with a squelched user bit still violates the boundary,
    // and the violation names the user bit alone.
    assert_eq!(
        squelched_bits(SIG_FUEL.union(SIG_YIELD), SIG_FUEL.union(SIG_YIELD)),
        SIG_YIELD
    );
}

// ── Filesystem capability ───────────────────────────────────────────

/// The filesystem primitives are synchronous `std::fs` calls. `:fs` marks the
/// authority; it must not claim the scheduler round trip that `:io` means.
#[test]
fn fs_errors_carries_authority_without_dispatch() {
    let s = Signal::fs_errors();
    assert!(s.bits.intersects(SIG_FS));
    assert!(s.may_error());
    assert!(
        !s.bits.intersects(SIG_IO),
        ":fs must not imply a scheduler round trip"
    );
    assert!(!s.may_yield(), ":fs must not imply suspension");
}

/// `port/open` resolves a path AND opens it through the scheduler, so either
/// denial must block it. Without `:fs` a fiber denied only the filesystem
/// could open a port on any path and read it.
#[test]
fn fs_io_yields_errors_is_deniable_from_either_side() {
    let s = Signal::fs_io_yields_errors();
    assert!(s.bits.intersects(SIG_FS));
    assert!(s.may_io());
    assert!(s.may_error());
    assert!(!s.may_yield(), "a request does not claim :yield");
}

/// The three async constructors share one base, so a change to what a
/// scheduler round trip means cannot reach one and miss the others.
#[test]
fn capability_gated_io_constructors_extend_the_same_base() {
    let base = Signal::io_yields_errors().bits;
    assert_eq!(Signal::subprocess().bits, base.union(SIG_EXEC));
    assert_eq!(Signal::fs_io_yields_errors().bits, base.union(SIG_FS));
}

/// A scheduler round trip does not claim `:yield`.
///
/// `:yield` is the cooperative suspension `(yield v)` raises and a `|:yield|`
/// mask catches. An I/O request suspends too, but suspension follows from
/// raising any signal (`signals::dispatch::is_suspending`), not from that bit —
/// so carrying it here would make one keyword mean two things, and a mask
/// naming it could not say which it wanted.
///
/// The counter-factual is a generator: `port/lines` masks `|:yield|` around a
/// body that calls `port/read-line`. With `:yield` on the request, that mask
/// swallows the read and it never reaches the scheduler
/// (tests/elle/io-request-carries-no-yield.lisp).
#[test]
fn a_scheduler_round_trip_does_not_carry_yield() {
    for sig in [
        Signal::io_yields_errors(),
        Signal::fs_io_yields_errors(),
        Signal::subprocess(),
    ] {
        assert!(
            !sig.bits.intersects(SIG_YIELD),
            "an async signal must not carry :yield — got {}",
            sig.bits
        );
        assert!(sig.bits.intersects(SIG_IO), "but it must carry :io");
    }
    // `(yield v)` still does, and is the only constructor that should.
    assert!(Signal::yields().bits.intersects(SIG_YIELD));
}

/// Every signal bit is a capability bit: a fiber mask can withhold `:fs`
/// exactly as it withholds `:io` or `:exec`.
#[test]
fn fs_is_deniable_and_not_vm_internal() {
    assert!(CAP_MASK.intersects(SIG_FS), ":fs must be deniable");
    assert!(!VM_INTERNAL.intersects(SIG_FS), ":fs is not VM-internal");
}

/// The keyword a `:deny |:fs|` mask is written with must resolve to the bit
/// the primitives declare, or the mask silently withholds nothing.
#[test]
fn fs_keyword_resolves_to_the_declared_bit() {
    let registry = registry::SignalRegistry::with_builtins();
    assert_eq!(registry.lookup("fs"), Some(SIG_FS.trailing_zeros()));
    assert_eq!(registry.to_signal_bits("fs"), Some(SIG_FS));
}
