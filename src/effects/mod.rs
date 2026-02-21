//! Effect system for tracking which expressions may yield and raise
//!
//! This module implements effect inference for colorless coroutines and
//! exception tracking. Effects track whether an expression may suspend
//! execution (yield) and whether it may raise an exception.

mod primitives;

pub use primitives::get_primitive_effects;

use std::collections::BTreeSet;
use std::fmt;

/// Yield behavior classification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum YieldBehavior {
    /// Expression never yields - can be compiled to native code
    #[default]
    Pure,
    /// Expression may yield - requires CPS transformation
    Yields,
    /// Effect depends on function parameters (for higher-order functions)
    /// The BTreeSet contains the indices of parameters whose effects this depends on (0-indexed)
    Polymorphic(BTreeSet<usize>),
}

/// Effect classification for expressions and functions.
///
/// Tracks two orthogonal axes:
/// - `yield_behavior`: whether the expression may yield (suspend)
/// - `may_raise`: whether the expression may raise an exception
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Effect {
    pub yield_behavior: YieldBehavior,
    pub may_raise: bool,
}

impl Effect {
    /// Pure effect: does not yield, does not raise
    pub const fn pure() -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Pure,
            may_raise: false,
        }
    }

    /// Pure but may raise (most primitives: arity/type errors)
    pub fn pure_raises() -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Pure,
            may_raise: true,
        }
    }

    /// May yield, does not raise
    pub fn yields() -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Yields,
            may_raise: false,
        }
    }

    /// May yield and may raise
    pub fn yields_raises() -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Yields,
            may_raise: true,
        }
    }

    /// Create a polymorphic effect depending on a single parameter (no raise)
    pub fn polymorphic(param: usize) -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Polymorphic(BTreeSet::from([param])),
            may_raise: false,
        }
    }

    /// Create a polymorphic effect depending on a single parameter (may raise)
    pub fn polymorphic_raises(param: usize) -> Effect {
        Effect {
            yield_behavior: YieldBehavior::Polymorphic(BTreeSet::from([param])),
            may_raise: true,
        }
    }

    /// Combine two effects (used for sequencing)
    /// Returns the "maximum" effect - if either yields, result yields.
    /// may_raise is ORed.
    pub fn combine(self, other: Effect) -> Effect {
        let yield_behavior = match (self.yield_behavior, other.yield_behavior) {
            (YieldBehavior::Pure, YieldBehavior::Pure) => YieldBehavior::Pure,
            (YieldBehavior::Yields, _) | (_, YieldBehavior::Yields) => YieldBehavior::Yields,
            (YieldBehavior::Polymorphic(s), YieldBehavior::Pure)
            | (YieldBehavior::Pure, YieldBehavior::Polymorphic(s)) => YieldBehavior::Polymorphic(s),
            (YieldBehavior::Polymorphic(mut a), YieldBehavior::Polymorphic(b)) => {
                a.extend(b);
                YieldBehavior::Polymorphic(a)
            }
        };
        Effect {
            yield_behavior,
            may_raise: self.may_raise || other.may_raise,
        }
    }

    /// Combine multiple effects
    pub fn combine_all(effects: impl IntoIterator<Item = Effect>) -> Effect {
        effects.into_iter().fold(Effect::pure(), Effect::combine)
    }

    /// Check if this effect is pure (no yield)
    pub fn is_pure(&self) -> bool {
        self.yield_behavior == YieldBehavior::Pure
    }

    /// Check if this effect may yield
    pub fn may_yield(&self) -> bool {
        self.yield_behavior == YieldBehavior::Yields
    }

    /// Check if this effect is polymorphic
    pub fn is_polymorphic(&self) -> bool {
        matches!(self.yield_behavior, YieldBehavior::Polymorphic(_))
    }
}

// Backward compatibility: allow comparing with the old enum-style patterns.
// These constants match the old Effect::pure(), Effect::yields() usage.
impl Effect {
    /// The old Effect::pure() equivalent
    pub const PURE: Effect = Effect {
        yield_behavior: YieldBehavior::Pure,
        may_raise: false,
    };

    /// The old Effect::yields() equivalent
    pub const YIELDS: Effect = Effect {
        yield_behavior: YieldBehavior::Yields,
        may_raise: false,
    };
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.yield_behavior {
            YieldBehavior::Pure => {
                if self.may_raise {
                    write!(f, "pure+raises")
                } else {
                    write!(f, "pure")
                }
            }
            YieldBehavior::Yields => {
                if self.may_raise {
                    write!(f, "yields+raises")
                } else {
                    write!(f, "yields")
                }
            }
            YieldBehavior::Polymorphic(params) => {
                let indices: Vec<_> = params.iter().map(|i| i.to_string()).collect();
                if self.may_raise {
                    write!(f, "polymorphic({})+raises", indices.join(","))
                } else {
                    write!(f, "polymorphic({})", indices.join(","))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_combine_pure() {
        assert_eq!(Effect::pure().combine(Effect::pure()), Effect::pure());
    }

    #[test]
    fn test_effect_combine_yields() {
        assert_eq!(Effect::pure().combine(Effect::yields()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::pure()), Effect::yields());
        assert_eq!(Effect::yields().combine(Effect::yields()), Effect::yields());
    }

    #[test]
    fn test_effect_combine_polymorphic() {
        assert_eq!(
            Effect::pure().combine(Effect::polymorphic(0)),
            Effect::polymorphic(0)
        );
        assert_eq!(
            Effect::polymorphic(1).combine(Effect::pure()),
            Effect::polymorphic(1)
        );
        assert_eq!(
            Effect::polymorphic(0).combine(Effect::yields()),
            Effect::yields()
        );
    }

    #[test]
    fn test_effect_combine_polymorphic_multiple() {
        let combined = Effect::polymorphic(0).combine(Effect::polymorphic(1));
        assert_eq!(
            combined,
            Effect {
                yield_behavior: YieldBehavior::Polymorphic(BTreeSet::from([0, 1])),
                may_raise: false,
            }
        );

        let combined2 = Effect::polymorphic(0).combine(Effect::polymorphic(0));
        assert_eq!(combined2, Effect::polymorphic(0));
    }

    #[test]
    fn test_effect_combine_all() {
        assert_eq!(
            Effect::combine_all([Effect::pure(), Effect::pure(), Effect::pure()]),
            Effect::pure()
        );
        assert_eq!(
            Effect::combine_all([Effect::pure(), Effect::yields(), Effect::pure()]),
            Effect::yields()
        );
    }

    #[test]
    fn test_effect_predicates() {
        assert!(Effect::pure().is_pure());
        assert!(!Effect::yields().is_pure());
        assert!(!Effect::polymorphic(0).is_pure());

        assert!(!Effect::pure().may_yield());
        assert!(Effect::yields().may_yield());

        assert!(!Effect::pure().is_polymorphic());
        assert!(Effect::polymorphic(0).is_polymorphic());
    }

    #[test]
    fn test_effect_may_raise() {
        assert!(!Effect::pure().may_raise);
        assert!(Effect::pure_raises().may_raise);
        assert!(!Effect::yields().may_raise);
        assert!(Effect::yields_raises().may_raise);

        // Combining raises
        let combined = Effect::pure().combine(Effect::pure_raises());
        assert!(combined.may_raise);
        assert!(combined.is_pure());

        // Both raise
        let combined2 = Effect::pure_raises().combine(Effect::pure_raises());
        assert!(combined2.may_raise);
    }

    #[test]
    fn test_effect_display() {
        assert_eq!(format!("{}", Effect::pure()), "pure");
        assert_eq!(format!("{}", Effect::yields()), "yields");
        assert_eq!(format!("{}", Effect::pure_raises()), "pure+raises");
        assert_eq!(format!("{}", Effect::yields_raises()), "yields+raises");
        assert_eq!(format!("{}", Effect::polymorphic(0)), "polymorphic(0)");
        assert_eq!(
            format!("{}", Effect::polymorphic_raises(0)),
            "polymorphic(0)+raises"
        );
    }

    // Backward compat constants
    #[test]
    fn test_backward_compat_constants() {
        assert_eq!(Effect::PURE, Effect::pure());
        assert_eq!(Effect::YIELDS, Effect::yields());
    }

    #[test]
    fn test_pure_raises_is_pure() {
        // A function that may raise but doesn't yield is considered pure
        // (is_pure only checks yield behavior, not raise behavior)
        assert!(
            Effect::pure_raises().is_pure(),
            "pure_raises should be pure (no yield)"
        );

        // Combining pure with pure_raises should still be pure
        let combined = Effect::pure().combine(Effect::pure_raises());
        assert!(
            combined.is_pure(),
            "pure combined with pure_raises should be pure (no yield)"
        );
    }
}
