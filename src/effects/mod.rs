//! Effect system for tracking which expressions may yield
//!
//! This module implements effect inference for colorless coroutines.
//! Effects track whether an expression may suspend execution (yield).

mod primitives;

pub use primitives::register_primitive_effects;

use std::fmt;

/// Effect classification for expressions and functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Effect {
    /// Expression never yields - can be compiled to native code
    #[default]
    Pure,
    /// Expression may yield - requires CPS transformation
    Yields,
    /// Effect depends on a function parameter (for higher-order functions)
    /// The usize indicates which parameter's effect this depends on (0-indexed)
    Polymorphic(usize),
}

impl Effect {
    /// Combine two effects (used for sequencing)
    /// Returns the "maximum" effect - if either yields, result yields
    pub fn combine(self, other: Effect) -> Effect {
        match (self, other) {
            (Effect::Pure, Effect::Pure) => Effect::Pure,
            (Effect::Yields, _) | (_, Effect::Yields) => Effect::Yields,
            (Effect::Polymorphic(i), Effect::Pure) => Effect::Polymorphic(i),
            (Effect::Pure, Effect::Polymorphic(i)) => Effect::Polymorphic(i),
            (Effect::Polymorphic(i), Effect::Polymorphic(j)) => {
                // Both polymorphic - conservatively use the first
                Effect::Polymorphic(i.min(j))
            }
        }
    }

    /// Combine multiple effects
    pub fn combine_all(effects: impl IntoIterator<Item = Effect>) -> Effect {
        effects.into_iter().fold(Effect::Pure, Effect::combine)
    }

    /// Check if this effect is pure
    pub fn is_pure(&self) -> bool {
        matches!(self, Effect::Pure)
    }

    /// Check if this effect may yield
    pub fn may_yield(&self) -> bool {
        matches!(self, Effect::Yields)
    }

    /// Check if this effect is polymorphic
    pub fn is_polymorphic(&self) -> bool {
        matches!(self, Effect::Polymorphic(_))
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Effect::Pure => write!(f, "pure"),
            Effect::Yields => write!(f, "yields"),
            Effect::Polymorphic(i) => write!(f, "polymorphic({})", i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_combine_pure() {
        assert_eq!(Effect::Pure.combine(Effect::Pure), Effect::Pure);
    }

    #[test]
    fn test_effect_combine_yields() {
        assert_eq!(Effect::Pure.combine(Effect::Yields), Effect::Yields);
        assert_eq!(Effect::Yields.combine(Effect::Pure), Effect::Yields);
        assert_eq!(Effect::Yields.combine(Effect::Yields), Effect::Yields);
    }

    #[test]
    fn test_effect_combine_polymorphic() {
        assert_eq!(
            Effect::Pure.combine(Effect::Polymorphic(0)),
            Effect::Polymorphic(0)
        );
        assert_eq!(
            Effect::Polymorphic(1).combine(Effect::Pure),
            Effect::Polymorphic(1)
        );
        assert_eq!(
            Effect::Polymorphic(0).combine(Effect::Yields),
            Effect::Yields
        );
    }

    #[test]
    fn test_effect_combine_all() {
        assert_eq!(
            Effect::combine_all([Effect::Pure, Effect::Pure, Effect::Pure]),
            Effect::Pure
        );
        assert_eq!(
            Effect::combine_all([Effect::Pure, Effect::Yields, Effect::Pure]),
            Effect::Yields
        );
        assert_eq!(
            Effect::combine_all([Effect::Pure, Effect::Polymorphic(0), Effect::Pure]),
            Effect::Polymorphic(0)
        );
    }

    #[test]
    fn test_effect_predicates() {
        assert!(Effect::Pure.is_pure());
        assert!(!Effect::Yields.is_pure());
        assert!(!Effect::Polymorphic(0).is_pure());

        assert!(!Effect::Pure.may_yield());
        assert!(Effect::Yields.may_yield());
        assert!(!Effect::Polymorphic(0).may_yield());

        assert!(!Effect::Pure.is_polymorphic());
        assert!(!Effect::Yields.is_polymorphic());
        assert!(Effect::Polymorphic(0).is_polymorphic());
    }

    #[test]
    fn test_effect_display() {
        assert_eq!(format!("{}", Effect::Pure), "pure");
        assert_eq!(format!("{}", Effect::Yields), "yields");
        assert_eq!(format!("{}", Effect::Polymorphic(0)), "polymorphic(0)");
    }
}
