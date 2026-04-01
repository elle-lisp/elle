//! Signal type for tracking which signals a function may emit.
//!
//! Signals are signal-bits-based: they track which signals a function
//! might emit (error, yield, debug, ffi, user-defined) and which
//! parameter indices propagate their callee's signals (for higher-order
//! functions like map/filter/fold).
//!
//! ## Compile-time vs. runtime signal representation
//!
//! `Signal` (this module) is a **compile-time** type used during HIR analysis
//! and LIR lowering. Its `propagates` field is a bitmask of parameter indices
//! whose signals flow through the function — this is needed to infer the signal
//! of a call site based on its arguments. `SignalBits` (in `value/fiber.rs`) is
//! the **runtime** representation: a flat bitmask stored on closures and used by
//! the VM and JIT for dispatch. These are intentionally separate types serving
//! different phases. The `propagates` field has no runtime analogue. Do not
//! attempt to unify them.

pub mod registry;

use crate::value::fiber::SignalBits;
use std::fmt;

// ---------------------------------------------------------------------------
// Signal constants — canonical definitions
// ---------------------------------------------------------------------------
//
// These are the semantic signal definitions for the signal system. They live
// here because the signal registry is the semantic owner; fiber.rs
// is a runtime data structure that consumes them.
//
// Signal bit partitioning:
//
//   Bits 0-2:   User-facing signals (error, yield, debug)
//   Bit  3:     Resume - run a suspended fiber (VM-internal)
//   Bit  4:     FFI — calls foreign code
//   Bit  5:     Propagate — propagate caught signal (VM-internal)
//   Bit  6:     Abort — graceful fiber termination with error injection (VM-internal)
//   Bit  7:     Query — read VM state without fiber swap (VM-internal)
//   Bit  8:     Halt — graceful VM termination with return value
//   Bit  9:     IO — I/O request to scheduler
//   Bit  10:    Terminal — non-resumable signal
//   Bit  11:    Exec — subprocess capability (access control; NOT a dispatch bit)
//   Bit  12:    Fuel — instruction budget exhaustion
//   Bit 13: SIG_SWITCH (fiber switch trampoline)
//   Bit 14: SIG_WAIT (structured concurrency wait request)
//   Bit 15: Reserved for future use
//   Bits 16-31: User-defined signal types

pub const SIG_OK: SignalBits = SignalBits::new(0); // no bits set = normal return
pub const SIG_ERROR: SignalBits = SignalBits::new(1 << 0); // exception / panic
pub const SIG_YIELD: SignalBits = SignalBits::new(1 << 1); // cooperative suspension
pub const SIG_DEBUG: SignalBits = SignalBits::new(1 << 2); // breakpoint / trace
pub const SIG_RESUME: SignalBits = SignalBits::new(1 << 3); // fiber resumption (VM-internal)
pub const SIG_FFI: SignalBits = SignalBits::new(1 << 4); // calls foreign code
pub const SIG_PROPAGATE: SignalBits = SignalBits::new(1 << 5); // propagate caught signal (VM-internal)
pub const SIG_ABORT: SignalBits = SIG_ERROR.union(SIG_TERMINAL); // graceful fiber termination with error injection (VM-internal)
pub const SIG_QUERY: SignalBits = SignalBits::new(1 << 7); // VM state query (VM-internal)
pub const SIG_HALT: SignalBits = SignalBits::new(1 << 8); // graceful VM termination
pub const SIG_IO: SignalBits = SignalBits::new(1 << 9); // I/O request to scheduler
pub const SIG_TERMINAL: SignalBits = SignalBits::new(1 << 10); // terminal signal (non-resumable)
pub const SIG_EXEC: SignalBits = SignalBits::new(1 << 11); // subprocess capability (capability bit, not dispatch)
pub const SIG_FUEL: SignalBits = SignalBits::new(1 << 12); // instruction budget exhaustion
pub const SIG_SWITCH: SignalBits = SignalBits::new(1 << 13); // fiber switch trampoline (VM-internal)
pub const SIG_WAIT: SignalBits = SignalBits::new(1 << 14); // structured concurrency wait request

/// Signal classification for expressions and functions.
///
/// Two fields:
/// - `bits`: which signals this function itself might emit
/// - `propagates`: bitmask of parameter indices whose signals this
///   function propagates (bit i set = parameter i's signals flow through)
///
/// `Copy` and `const fn` constructors — no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signal {
    /// Signal bits this function itself might emit.
    pub bits: SignalBits,
    /// Bitmask of parameter indices whose signals this function propagates.
    /// Bit i set means this function may exhibit parameter i's signals.
    pub propagates: u32,
}

impl Default for Signal {
    fn default() -> Self {
        Signal::silent()
    }
}

// ── Constructors ────────────────────────────────────────────────────

impl Signal {
    /// No signals: does not signal, does not propagate.
    pub const fn silent() -> Self {
        Signal {
            bits: SignalBits::new(0),
            propagates: 0,
        }
    }

    /// May error (most primitives: arity/type errors).
    pub const fn errors() -> Self {
        Signal {
            bits: SIG_ERROR,
            propagates: 0,
        }
    }

    /// May yield (cooperative suspension).
    pub const fn yields() -> Self {
        Signal {
            bits: SIG_YIELD,
            propagates: 0,
        }
    }

    /// May yield and may error.
    pub const fn yields_errors() -> Self {
        Signal {
            bits: SIG_YIELD.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// May halt the VM (non-resumable termination with return value).
    pub const fn halts() -> Self {
        Signal {
            bits: SIG_HALT.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Calls foreign code via FFI.
    pub const fn ffi() -> Self {
        Signal {
            bits: SIG_FFI,
            propagates: 0,
        }
    }

    /// Calls foreign code and may error (SIG_FFI | SIG_ERROR).
    /// Used for FFI primitives that validate arguments before calling C.
    pub const fn ffi_errors() -> Self {
        Signal {
            bits: SIG_FFI.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Polymorphic: signal depends on a single parameter (no error signal).
    pub const fn polymorphic(param: usize) -> Self {
        Signal {
            bits: SignalBits::new(0),
            propagates: 1 << param,
        }
    }

    /// Polymorphic: signal depends on a single parameter (may error).
    pub const fn polymorphic_errors(param: usize) -> Self {
        Signal {
            bits: SIG_ERROR,
            propagates: 1 << param,
        }
    }

    /// Combine two signals (used for sequencing).
    /// Signal bits are ORed. Propagation masks are ORed.
    pub const fn combine(self, other: Signal) -> Signal {
        Signal {
            bits: self.bits.union(other.bits),
            propagates: self.propagates | other.propagates,
        }
    }

    /// Combine multiple signals.
    pub fn combine_all(signals: impl IntoIterator<Item = Signal>) -> Signal {
        signals
            .into_iter()
            .fold(Signal::silent(), |a, b| a.combine(b))
    }
}

// ── Predicates ──────────────────────────────────────────────────────
//
// Each predicate asks a specific question about capabilities.

impl Signal {
    /// Can this function suspend execution?
    /// Suspension signals: yield, debug. Polymorphic signals may also
    /// suspend (depends on the argument's signal at the call site).
    pub const fn may_suspend(&self) -> bool {
        self.bits.intersects(SIG_YIELD.union(SIG_DEBUG)) || self.propagates != 0
    }

    /// Can this function yield (cooperative suspension)?
    pub const fn may_yield(&self) -> bool {
        self.bits.intersects(SIG_YIELD)
    }

    /// Can this function error?
    pub const fn may_error(&self) -> bool {
        self.bits.intersects(SIG_ERROR)
    }

    /// Can this function halt the VM?
    pub const fn may_halt(&self) -> bool {
        self.bits.intersects(SIG_HALT)
    }

    /// Does this function call foreign code?
    pub const fn may_ffi(&self) -> bool {
        self.bits.intersects(SIG_FFI)
    }

    /// Can this function perform I/O?
    pub const fn may_io(&self) -> bool {
        self.bits.intersects(SIG_IO)
    }

    /// Does this function's signal depend on its arguments?
    pub const fn is_polymorphic(&self) -> bool {
        self.propagates != 0
    }

    /// Get the set of parameter indices this signal propagates.
    pub fn propagated_params(&self) -> impl Iterator<Item = usize> {
        let mask = self.propagates;
        (0..32).filter(move |i| mask & (1 << i) != 0)
    }
}

// ── Constants ───────────────────────────────────────────────────────

impl Signal {
    pub const SILENT: Signal = Signal::silent();
    pub const YIELDS: Signal = Signal::yields();
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.propagates != 0 {
            let indices: Vec<_> = self.propagated_params().map(|i| i.to_string()).collect();
            write!(f, "polymorphic({})", indices.join(","))?;
        } else if self.bits.contains(SIG_YIELD) {
            write!(f, "yields")?;
        } else {
            write!(f, "silent")?;
        }

        // Append capability flags
        let mut flags = Vec::new();
        if self.bits.contains(SIG_ERROR) {
            flags.push("errors");
        }
        if self.bits.contains(SIG_HALT) {
            flags.push("halts");
        }
        if self.bits.contains(SIG_FFI) {
            flags.push("ffi");
        }
        if self.bits.contains(SIG_DEBUG) {
            flags.push("debug");
        }
        if !flags.is_empty() {
            write!(f, "+{}", flags.join("+"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        assert!(!Signal::errors().may_suspend());
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
        assert!(!combined.may_suspend());
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
        assert!(!e.may_suspend());
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
}
