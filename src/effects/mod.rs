//! Effect system for tracking which signals a function may emit.
//!
//! Effects are signal-bits-based: they track which signals a function
//! might emit (error, yield, debug, ffi, user-defined) and which
//! parameter indices propagate their callee's effects (for higher-order
//! functions like map/filter/fold).

use crate::value::fiber::SignalBits;
use crate::value::fiber::{SIG_DEBUG, SIG_ERROR, SIG_FFI, SIG_HALT, SIG_IO, SIG_YIELD};
use std::fmt;

/// Effect classification for expressions and functions.
///
/// Two fields:
/// - `bits`: which signals this function itself might emit
/// - `propagates`: bitmask of parameter indices whose effects this
///   function propagates (bit i set = parameter i's effects flow through)
///
/// `Copy` and `const fn` constructors — no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Effect {
    /// Signal bits this function itself might emit.
    pub bits: SignalBits,
    /// Bitmask of parameter indices whose effects this function propagates.
    /// Bit i set means this function may exhibit parameter i's effects.
    pub propagates: u32,
}

impl Default for Effect {
    fn default() -> Self {
        Effect::inert()
    }
}

// ── Constructors ────────────────────────────────────────────────────

impl Effect {
    /// No effects: does not signal, does not propagate.
    pub const fn inert() -> Self {
        Effect {
            bits: SignalBits::new(0),
            propagates: 0,
        }
    }

    /// May error (most primitives: arity/type errors).
    pub const fn errors() -> Self {
        Effect {
            bits: SIG_ERROR,
            propagates: 0,
        }
    }

    /// May yield (cooperative suspension).
    pub const fn yields() -> Self {
        Effect {
            bits: SIG_YIELD,
            propagates: 0,
        }
    }

    /// May yield and may error.
    pub const fn yields_errors() -> Self {
        Effect {
            bits: SignalBits::new(SIG_YIELD.0 | SIG_ERROR.0),
            propagates: 0,
        }
    }

    /// May halt the VM (non-resumable termination with return value).
    pub const fn halts() -> Self {
        Effect {
            bits: SignalBits::new(SIG_HALT.0 | SIG_ERROR.0),
            propagates: 0,
        }
    }

    /// Calls foreign code via FFI.
    pub const fn ffi() -> Self {
        Effect {
            bits: SIG_FFI,
            propagates: 0,
        }
    }

    /// Calls foreign code and may error (SIG_FFI | SIG_ERROR).
    /// Used for FFI primitives that validate arguments before calling C.
    pub const fn ffi_errors() -> Self {
        Effect {
            bits: SignalBits::new(SIG_FFI.0 | SIG_ERROR.0),
            propagates: 0,
        }
    }

    /// Polymorphic: effect depends on a single parameter (no error signal).
    pub const fn polymorphic(param: usize) -> Self {
        Effect {
            bits: SignalBits::new(0),
            propagates: 1 << param,
        }
    }

    /// Polymorphic: effect depends on a single parameter (may error).
    pub const fn polymorphic_errors(param: usize) -> Self {
        Effect {
            bits: SIG_ERROR,
            propagates: 1 << param,
        }
    }

    /// Combine two effects (used for sequencing).
    /// Signal bits are ORed. Propagation masks are ORed.
    pub const fn combine(self, other: Effect) -> Effect {
        Effect {
            bits: SignalBits::new(self.bits.0 | other.bits.0),
            propagates: self.propagates | other.propagates,
        }
    }

    /// Combine multiple effects.
    pub fn combine_all(effects: impl IntoIterator<Item = Effect>) -> Effect {
        effects
            .into_iter()
            .fold(Effect::inert(), |a, b| a.combine(b))
    }
}

// ── Predicates ──────────────────────────────────────────────────────
//
// Each predicate asks a specific question about capabilities.

impl Effect {
    /// Can this function suspend execution?
    /// Suspension signals: yield, debug. Polymorphic effects may also
    /// suspend (depends on the argument's effect at the call site).
    pub const fn may_suspend(&self) -> bool {
        const SUSPENSION_BITS: u32 = SIG_YIELD.0 | SIG_DEBUG.0;
        (self.bits.0 & SUSPENSION_BITS) != 0 || self.propagates != 0
    }

    /// Can this function yield (cooperative suspension)?
    pub const fn may_yield(&self) -> bool {
        self.bits.0 & SIG_YIELD.0 != 0
    }

    /// Can this function error?
    pub const fn may_error(&self) -> bool {
        self.bits.0 & SIG_ERROR.0 != 0
    }

    /// Can this function halt the VM?
    pub const fn may_halt(&self) -> bool {
        self.bits.0 & SIG_HALT.0 != 0
    }

    /// Does this function call foreign code?
    pub const fn may_ffi(&self) -> bool {
        self.bits.0 & SIG_FFI.0 != 0
    }

    /// Can this function perform I/O?
    pub const fn may_io(&self) -> bool {
        self.bits.0 & SIG_IO.0 != 0
    }

    /// Does this function's effect depend on its arguments?
    pub const fn is_polymorphic(&self) -> bool {
        self.propagates != 0
    }

    /// Get the set of parameter indices this effect propagates.
    pub fn propagated_params(&self) -> impl Iterator<Item = usize> {
        let mask = self.propagates;
        (0..32).filter(move |i| mask & (1 << i) != 0)
    }
}

// ── Constants ───────────────────────────────────────────────────────

impl Effect {
    pub const INERT: Effect = Effect::inert();
    pub const YIELDS: Effect = Effect::yields();
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.propagates != 0 {
            let indices: Vec<_> = self.propagated_params().map(|i| i.to_string()).collect();
            write!(f, "polymorphic({})", indices.join(","))?;
        } else if self.bits.contains(SIG_YIELD) {
            write!(f, "yields")?;
        } else {
            write!(f, "inert")?;
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
    fn test_effect_combine_inert() {
        assert_eq!(Effect::inert().combine(Effect::inert()), Effect::inert());
    }

    #[test]
    fn test_effect_combine_yields() {
        assert_eq!(Effect::inert().combine(Effect::yields()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::inert()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::yields()), Effect::yields());
    }

    #[test]
    fn test_effect_combine_polymorphic() {
        assert_eq!(
            Effect::inert().combine(Effect::polymorphic(0)),
            Effect::polymorphic(0)
        );
        assert_eq!(
            Effect::polymorphic(1).combine(Effect::inert()),
            Effect::polymorphic(1)
        );
        // Polymorphic + Yields = both
        let combined = Effect::polymorphic(0).combine(Effect::yields());
        assert!(combined.may_yield());
        assert!(combined.is_polymorphic());
    }

    #[test]
    fn test_effect_combine_polymorphic_multiple() {
        let combined = Effect::polymorphic(0).combine(Effect::polymorphic(1));
        assert_eq!(
            combined,
            Effect {
                bits: SignalBits::new(0),
                propagates: 0b11,
            }
        );

        let combined2 = Effect::polymorphic(0).combine(Effect::polymorphic(0));
        assert_eq!(combined2, Effect::polymorphic(0));
    }

    #[test]
    fn test_effect_combine_all() {
        assert_eq!(
            Effect::combine_all([Effect::inert(), Effect::inert(), Effect::inert()]),
            Effect::inert()
        );
        assert_eq!(
            Effect::combine_all([Effect::inert(), Effect::yields(), Effect::inert()]),
            Effect::yields()
        );
    }

    #[test]
    fn test_may_suspend() {
        assert!(!Effect::inert().may_suspend());
        assert!(!Effect::errors().may_suspend());
        assert!(Effect::yields().may_suspend());
        assert!(Effect::polymorphic(0).may_suspend());
        assert!(Effect {
            bits: SIG_DEBUG,
            propagates: 0,
        }
        .may_suspend());
    }

    #[test]
    fn test_may_yield() {
        assert!(!Effect::inert().may_yield());
        assert!(Effect::yields().may_yield());
        assert!(!Effect::errors().may_yield());
    }

    #[test]
    fn test_may_error() {
        assert!(!Effect::inert().may_error());
        assert!(Effect::errors().may_error());
        assert!(!Effect::yields().may_error());
        assert!(Effect::yields_errors().may_error());

        // Combining errors
        let combined = Effect::inert().combine(Effect::errors());
        assert!(combined.may_error());
        assert!(!combined.may_suspend());
    }

    #[test]
    fn test_may_ffi() {
        assert!(!Effect::inert().may_ffi());
        assert!(Effect::ffi().may_ffi());
        assert!(Effect::ffi_errors().may_ffi());
    }

    #[test]
    fn test_ffi_errors() {
        let e = Effect::ffi_errors();
        assert!(e.may_ffi());
        assert!(e.may_error());
        assert!(!e.may_yield());
        assert!(!e.may_suspend());
        assert!(!e.is_polymorphic());
    }

    #[test]
    fn test_is_polymorphic() {
        assert!(!Effect::inert().is_polymorphic());
        assert!(Effect::polymorphic(0).is_polymorphic());
    }

    #[test]
    fn test_effect_display() {
        assert_eq!(format!("{}", Effect::inert()), "inert");
        assert_eq!(format!("{}", Effect::yields()), "yields");
        assert_eq!(format!("{}", Effect::errors()), "inert+errors");
        assert_eq!(format!("{}", Effect::yields_errors()), "yields+errors");
        assert_eq!(format!("{}", Effect::polymorphic(0)), "polymorphic(0)");
        assert_eq!(
            format!("{}", Effect::polymorphic_errors(0)),
            "polymorphic(0)+errors"
        );
        assert_eq!(format!("{}", Effect::ffi()), "inert+ffi");
        assert_eq!(format!("{}", Effect::ffi_errors()), "inert+errors+ffi");
    }

    #[test]
    fn test_propagated_params() {
        let e = Effect {
            bits: SignalBits::new(0),
            propagates: 0b101, // params 0 and 2
        };
        let params: Vec<_> = e.propagated_params().collect();
        assert_eq!(params, vec![0, 2]);
    }

    #[test]
    fn test_effect_is_copy() {
        let e = Effect::yields();
        let e2 = e; // Copy
        assert_eq!(e, e2);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Effect::INERT, Effect::inert());
        assert_eq!(Effect::YIELDS, Effect::yields());
    }
}
