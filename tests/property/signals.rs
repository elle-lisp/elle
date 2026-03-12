// Unit tests for signal combine laws and signal predicates.
//
// Verifies that the signal system satisfies algebraic laws:
// - Signal combine is commutative, associative, and idempotent
// - Signal::inert() is the identity element
// - Propagates field is correctly ORed during combine
// - Signal predicates (may_yield, may_error, may_suspend, etc.) work correctly
// Converted from property tests to deterministic unit tests with concrete cases.

use elle::signals::Signal;
use elle::value::SignalBits;

// =========================================================================
// Signal combine laws: commutativity
// =========================================================================

#[test]
fn signal_combine_commutative_none_none() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    assert_eq!(a.combine(b), b.combine(a));
}

#[test]
fn signal_combine_commutative_1_2() {
    let a = Signal {
        bits: SignalBits(1),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(2),
        propagates: 0,
    };
    assert_eq!(a.combine(b), b.combine(a));
}

#[test]
fn signal_combine_commutative_3_5() {
    let a = Signal {
        bits: SignalBits(3),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(5),
        propagates: 0,
    };
    assert_eq!(a.combine(b), b.combine(a));
}

#[test]
fn signal_combine_commutative_7_7() {
    let a = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(a.combine(b), b.combine(a));
}

// =========================================================================
// Signal combine laws: associativity
// =========================================================================

#[test]
fn signal_combine_associative_none_none_none() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    let c = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    assert_eq!(a.combine(b).combine(c), a.combine(b.combine(c)));
}

#[test]
fn signal_combine_associative_1_2_4() {
    let a = Signal {
        bits: SignalBits(1),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(2),
        propagates: 0,
    };
    let c = Signal {
        bits: SignalBits(4),
        propagates: 0,
    };
    assert_eq!(a.combine(b).combine(c), a.combine(b.combine(c)));
}

#[test]
fn signal_combine_associative_3_5_7() {
    let a = Signal {
        bits: SignalBits(3),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(5),
        propagates: 0,
    };
    let c = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(a.combine(b).combine(c), a.combine(b.combine(c)));
}

#[test]
fn signal_combine_associative_all_same() {
    let a = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    let c = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(a.combine(b).combine(c), a.combine(b.combine(c)));
}

// =========================================================================
// Signal combine laws: identity
// =========================================================================

#[test]
fn signal_combine_identity_none_right() {
    let e = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    assert_eq!(e.combine(Signal::inert()), e);
}

#[test]
fn signal_combine_identity_none_left() {
    let e = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    assert_eq!(Signal::inert().combine(e), e);
}

#[test]
fn signal_combine_identity_1_right() {
    let e = Signal {
        bits: SignalBits(1),
        propagates: 0,
    };
    assert_eq!(e.combine(Signal::inert()), e);
}

#[test]
fn signal_combine_identity_1_left() {
    let e = Signal {
        bits: SignalBits(1),
        propagates: 0,
    };
    assert_eq!(Signal::inert().combine(e), e);
}

#[test]
fn signal_combine_identity_7_right() {
    let e = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(e.combine(Signal::inert()), e);
}

#[test]
fn signal_combine_identity_7_left() {
    let e = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(Signal::inert().combine(e), e);
}

#[test]
fn signal_combine_identity_15_right() {
    let e = Signal {
        bits: SignalBits(15),
        propagates: 0,
    };
    assert_eq!(e.combine(Signal::inert()), e);
}

#[test]
fn signal_combine_identity_15_left() {
    let e = Signal {
        bits: SignalBits(15),
        propagates: 0,
    };
    assert_eq!(Signal::inert().combine(e), e);
}

// =========================================================================
// Signal combine laws: idempotence
// =========================================================================

#[test]
fn signal_combine_idempotent_none() {
    let e = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

#[test]
fn signal_combine_idempotent_1() {
    let e = Signal {
        bits: SignalBits(1),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

#[test]
fn signal_combine_idempotent_3() {
    let e = Signal {
        bits: SignalBits(3),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

#[test]
fn signal_combine_idempotent_5() {
    let e = Signal {
        bits: SignalBits(5),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

#[test]
fn signal_combine_idempotent_7() {
    let e = Signal {
        bits: SignalBits(7),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

#[test]
fn signal_combine_idempotent_15() {
    let e = Signal {
        bits: SignalBits(15),
        propagates: 0,
    };
    assert_eq!(e.combine(e), e);
}

// =========================================================================
// Signal propagates: OR combination
// =========================================================================

#[test]
fn signal_propagates_combine_none_none() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 0,
    };
    let combined = a.combine(b);
    assert_eq!(combined.propagates, 0);
}

#[test]
fn signal_propagates_combine_1_2() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 1,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 2,
    };
    let combined = a.combine(b);
    assert_eq!(combined.propagates, 1 | 2);
}

#[test]
fn signal_propagates_combine_128_255() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 128,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 255,
    };
    let combined = a.combine(b);
    assert_eq!(combined.propagates, 128 | 255);
}

#[test]
fn signal_propagates_combine_same() {
    let a = Signal {
        bits: SignalBits(0),
        propagates: 42,
    };
    let b = Signal {
        bits: SignalBits(0),
        propagates: 42,
    };
    let combined = a.combine(b);
    assert_eq!(combined.propagates, 42 | 42);
}

// =========================================================================
// Polymorphic signals
// =========================================================================

#[test]
fn polymorphic_signal_is_polymorphic_0() {
    let signal = Signal::polymorphic(0);
    assert!(signal.is_polymorphic());
    assert!(signal.may_suspend());
}

#[test]
fn polymorphic_signal_is_polymorphic_1() {
    let signal = Signal::polymorphic(1);
    assert!(signal.is_polymorphic());
    assert!(signal.may_suspend());
}

#[test]
fn polymorphic_signal_is_polymorphic_7() {
    let signal = Signal::polymorphic(7);
    assert!(signal.is_polymorphic());
    assert!(signal.may_suspend());
}

#[test]
fn polymorphic_propagates_correct_param_0() {
    let signal = Signal::polymorphic(0);
    let propagated: Vec<_> = signal.propagated_params().collect();
    assert_eq!(propagated.len(), 1);
    assert_eq!(propagated[0], 0);
}

#[test]
fn polymorphic_propagates_correct_param_1() {
    let signal = Signal::polymorphic(1);
    let propagated: Vec<_> = signal.propagated_params().collect();
    assert_eq!(propagated.len(), 1);
    assert_eq!(propagated[0], 1);
}

#[test]
fn polymorphic_propagates_correct_param_7() {
    let signal = Signal::polymorphic(7);
    let propagated: Vec<_> = signal.propagated_params().collect();
    assert_eq!(propagated.len(), 1);
    assert_eq!(propagated[0], 7);
}

#[test]
fn polymorphic_errors_has_error_bit_0() {
    let signal = Signal::polymorphic_errors(0);
    assert!(signal.may_error());
    assert!(signal.is_polymorphic());
}

#[test]
fn polymorphic_errors_has_error_bit_1() {
    let signal = Signal::polymorphic_errors(1);
    assert!(signal.may_error());
    assert!(signal.is_polymorphic());
}

#[test]
fn polymorphic_errors_has_error_bit_7() {
    let signal = Signal::polymorphic_errors(7);
    assert!(signal.may_error());
    assert!(signal.is_polymorphic());
}

// =========================================================================
// Signal predicates
// =========================================================================

#[test]
fn none_signal_is_not_yielding() {
    let signal = Signal::inert();
    assert!(!signal.may_yield());
    assert!(!signal.may_ffi());
    assert!(!signal.may_suspend());
}

#[test]
fn yields_signal_may_yield() {
    let signal = Signal::yields();
    assert!(signal.may_yield());
    assert!(signal.may_suspend());
}

#[test]
fn errors_signal_may_error() {
    let signal = Signal::errors();
    assert!(signal.may_error());
    assert!(!signal.may_yield());
}

#[test]
fn yields_errors_has_both() {
    let signal = Signal::yields_errors();
    assert!(signal.may_yield());
    assert!(signal.may_error());
    assert!(signal.may_suspend());
}

#[test]
fn ffi_signal_may_ffi() {
    let signal = Signal::ffi();
    assert!(signal.may_ffi());
}

#[test]
fn halts_signal_may_halt() {
    let signal = Signal::halts();
    assert!(signal.may_halt());
    assert!(signal.may_error());
}
