//! Effect system for tracking which signals a function may emit.
//!
//! Effects are signal-bits-based: they track which signals a function
//! might emit (error, yield, debug, ffi, user-defined) and which
//! parameter indices propagate their callee's effects (for higher-order
//! functions like map/filter/fold).

mod primitives;

pub use primitives::get_primitive_effects;

use crate::value::fiber::SignalBits;
use crate::value::fiber::{SIG_DEBUG, SIG_ERROR, SIG_FFI, SIG_YIELD};
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
        Effect::none()
    }
}

// ── Constructors ────────────────────────────────────────────────────

impl Effect {
    /// No effects: does not signal, does not propagate.
    pub const fn none() -> Self {
        Effect {
            bits: 0,
            propagates: 0,
        }
    }

    /// May raise an error (most primitives: arity/type errors).
    pub const fn raises() -> Self {
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

    /// May yield and may raise.
    pub const fn yields_raises() -> Self {
        Effect {
            bits: SIG_YIELD | SIG_ERROR,
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

    /// Polymorphic: effect depends on a single parameter (no raise).
    pub const fn polymorphic(param: usize) -> Self {
        Effect {
            bits: 0,
            propagates: 1 << param,
        }
    }

    /// Polymorphic: effect depends on a single parameter (may raise).
    pub const fn polymorphic_raises(param: usize) -> Self {
        Effect {
            bits: SIG_ERROR,
            propagates: 1 << param,
        }
    }

    /// Combine two effects (used for sequencing).
    /// Signal bits are ORed. Propagation masks are ORed.
    pub const fn combine(self, other: Effect) -> Effect {
        Effect {
            bits: self.bits | other.bits,
            propagates: self.propagates | other.propagates,
        }
    }

    /// Combine multiple effects.
    pub fn combine_all(effects: impl IntoIterator<Item = Effect>) -> Effect {
        effects
            .into_iter()
            .fold(Effect::none(), |a, b| a.combine(b))
    }
}

// ── Predicates ──────────────────────────────────────────────────────
//
// Each predicate asks a specific question. No vague "is_pure".

impl Effect {
    /// Can this function suspend execution?
    /// Suspension signals: yield, debug. Polymorphic effects may also
    /// suspend (depends on the argument's effect at the call site).
    pub const fn may_suspend(&self) -> bool {
        const SUSPENSION_BITS: SignalBits = SIG_YIELD | SIG_DEBUG;
        (self.bits & SUSPENSION_BITS) != 0 || self.propagates != 0
    }

    /// Can this function yield (cooperative suspension)?
    pub const fn may_yield(&self) -> bool {
        self.bits & SIG_YIELD != 0
    }

    /// Can this function raise an error?
    pub const fn may_raise(&self) -> bool {
        self.bits & SIG_ERROR != 0
    }

    /// Does this function call foreign code?
    pub const fn may_ffi(&self) -> bool {
        self.bits & SIG_FFI != 0
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

// ── Backward compatibility ──────────────────────────────────────────

impl Effect {
    /// Alias for `none()`. Deprecated — use `none()` or a specific
    /// constructor instead.
    pub const fn pure() -> Self {
        Self::none()
    }

    /// Alias for `raises()`. Deprecated — use `raises()` directly.
    pub const fn pure_raises() -> Self {
        Self::raises()
    }

    /// Deprecated — use `!may_suspend()` or check specific capabilities.
    pub const fn is_pure(&self) -> bool {
        !self.may_suspend()
    }

    pub const PURE: Effect = Effect::none();
    pub const YIELDS: Effect = Effect::yields();
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.propagates != 0 {
            let indices: Vec<_> = self.propagated_params().map(|i| i.to_string()).collect();
            write!(f, "polymorphic({})", indices.join(","))?;
        } else if self.bits & SIG_YIELD != 0 {
            write!(f, "yields")?;
        } else {
            write!(f, "none")?;
        }

        // Append capability flags
        let mut flags = Vec::new();
        if self.bits & SIG_ERROR != 0 {
            flags.push("raises");
        }
        if self.bits & SIG_FFI != 0 {
            flags.push("ffi");
        }
        if self.bits & SIG_DEBUG != 0 {
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
    fn test_effect_combine_none() {
        assert_eq!(Effect::none().combine(Effect::none()), Effect::none());
    }

    #[test]
    fn test_effect_combine_yields() {
        assert_eq!(Effect::none().combine(Effect::yields()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::none()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::yields()), Effect::yields());
    }

    #[test]
    fn test_effect_combine_polymorphic() {
        assert_eq!(
            Effect::none().combine(Effect::polymorphic(0)),
            Effect::polymorphic(0)
        );
        assert_eq!(
            Effect::polymorphic(1).combine(Effect::none()),
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
                bits: 0,
                propagates: 0b11,
            }
        );

        let combined2 = Effect::polymorphic(0).combine(Effect::polymorphic(0));
        assert_eq!(combined2, Effect::polymorphic(0));
    }

    #[test]
    fn test_effect_combine_all() {
        assert_eq!(
            Effect::combine_all([Effect::none(), Effect::none(), Effect::none()]),
            Effect::none()
        );
        assert_eq!(
            Effect::combine_all([Effect::none(), Effect::yields(), Effect::none()]),
            Effect::yields()
        );
    }

    #[test]
    fn test_may_suspend() {
        assert!(!Effect::none().may_suspend());
        assert!(!Effect::raises().may_suspend());
        assert!(Effect::yields().may_suspend());
        assert!(Effect::polymorphic(0).may_suspend());
        assert!(Effect {
            bits: SIG_DEBUG,
            propagates: 0
        }
        .may_suspend());
    }

    #[test]
    fn test_may_yield() {
        assert!(!Effect::none().may_yield());
        assert!(Effect::yields().may_yield());
        assert!(!Effect::raises().may_yield());
    }

    #[test]
    fn test_may_raise() {
        assert!(!Effect::none().may_raise());
        assert!(Effect::raises().may_raise());
        assert!(!Effect::yields().may_raise());
        assert!(Effect::yields_raises().may_raise());

        // Combining raises
        let combined = Effect::none().combine(Effect::raises());
        assert!(combined.may_raise());
        assert!(!combined.may_suspend());
    }

    #[test]
    fn test_may_ffi() {
        assert!(!Effect::none().may_ffi());
        assert!(Effect::ffi().may_ffi());
    }

    #[test]
    fn test_is_polymorphic() {
        assert!(!Effect::none().is_polymorphic());
        assert!(Effect::polymorphic(0).is_polymorphic());
    }

    #[test]
    fn test_effect_display() {
        assert_eq!(format!("{}", Effect::none()), "none");
        assert_eq!(format!("{}", Effect::yields()), "yields");
        assert_eq!(format!("{}", Effect::raises()), "none+raises");
        assert_eq!(format!("{}", Effect::yields_raises()), "yields+raises");
        assert_eq!(format!("{}", Effect::polymorphic(0)), "polymorphic(0)");
        assert_eq!(
            format!("{}", Effect::polymorphic_raises(0)),
            "polymorphic(0)+raises"
        );
        assert_eq!(format!("{}", Effect::ffi()), "none+ffi");
    }

    #[test]
    fn test_propagated_params() {
        let e = Effect {
            bits: 0,
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
    fn test_backward_compat() {
        assert_eq!(Effect::pure(), Effect::none());
        assert_eq!(Effect::pure_raises(), Effect::raises());
        assert_eq!(Effect::PURE, Effect::none());
        assert_eq!(Effect::YIELDS, Effect::yields());
        // is_pure() = !may_suspend()
        assert!(Effect::none().is_pure());
        assert!(Effect::raises().is_pure()); // raises doesn't suspend
        assert!(!Effect::yields().is_pure());
        assert!(!Effect::polymorphic(0).is_pure());
    }
}
