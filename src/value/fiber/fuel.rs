// audited: 2026-10-12
//! `Fuel`: a fiber's instruction budget, and the charge each charging
//! instruction takes from it.
//!
//! docs/runtime.md

/// A fiber's instruction budget: a count of units left, or no budget at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fuel(Option<u32>);

impl Fuel {
    /// No budget: the fiber runs until it finishes or suspends.
    pub const fn unmetered() -> Self {
        Fuel(None)
    }

    /// A budget of `n` units.
    pub fn budget(n: u64) -> Option<Self> {
        Some(Fuel(Some(n as u32)))
    }

    /// The units left, or `None` for a fiber with no budget.
    pub fn remaining(self) -> Option<u64> {
        self.0.map(u64::from)
    }

    /// Whether the fiber has a budget at all.
    #[inline]
    pub fn is_metered(self) -> bool {
        self.0.is_some()
    }

    /// Take one unit if the budget holds one, and never refuse: the charge a
    /// silent function makes, which spends fuel and never pauses
    /// (docs/signals/inference.md).
    #[inline]
    pub fn spend(&mut self) {}

    /// Take one unit. Answers false, and takes nothing, when the budget is
    /// spent: the charging instruction must then pause the fiber.
    #[inline]
    pub fn charge(&mut self) -> bool {
        match &mut self.0 {
            Some(0) => false,
            Some(n) => {
                *n -= 1;
                true
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod tests;
