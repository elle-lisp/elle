// audited: 2026-10-12
//! `Fuel`: a fiber's instruction budget, and the charge each charging
//! instruction takes from it.
//!
//! docs/runtime.md
//! docs/impl/jit.md
//!
//! The budget is one 64-bit word, and an unmetered fiber holds `u64::MAX`
//! there, so a charge is a load, a test for zero and a store on either tier,
//! and a silent function's spend is a load and a store.
//! Compiled code charges the word in place; [`Fuel::WORD_OFFSET_IN_VM`] is
//! where it finds it.

/// An instruction budget: a count of units left, or no budget at all. Each fiber
/// holds one, and so does the expansion budget a VM keeps (`ExpansionBudget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Fuel(u64);

impl Fuel {
    /// The largest budget: the largest integer a program can write. Any word
    /// above it is a fiber with no budget.
    const LARGEST: u64 = i64::MAX as u64;

    /// The byte offset of the budget word from the start of a `VM`, which is
    /// what compiled code charges through its VM pointer.
    #[cfg(feature = "jit")]
    pub(crate) const WORD_OFFSET_IN_VM: usize =
        std::mem::offset_of!(crate::vm::VM, fiber) + std::mem::offset_of!(super::Fiber, fuel);

    /// No budget: the fiber runs until it finishes or suspends.
    pub const fn unmetered() -> Self {
        Fuel(u64::MAX)
    }

    /// A budget of `n` units, or `None` when `n` is past the largest integer.
    pub fn budget(n: u64) -> Option<Self> {
        (n <= Self::LARGEST).then_some(Fuel(n))
    }

    /// The units left, or `None` for a fiber with no budget.
    pub fn remaining(self) -> Option<u64> {
        (self.0 <= Self::LARGEST).then_some(self.0)
    }

    /// Whether the fiber has a budget at all.
    #[inline]
    pub fn is_metered(self) -> bool {
        self.0 <= Self::LARGEST
    }

    /// Take one unit if the budget holds one, and never refuse: the charge a
    /// silent function makes, which spends fuel and never pauses
    /// (docs/signals/inference.md).
    #[inline]
    pub fn spend(&mut self) {
        self.0 -= u64::from(self.0 != 0);
    }

    /// Take one unit. Answers false, and takes nothing, when the budget is
    /// spent: the charging instruction must then pause the fiber.
    #[inline]
    pub fn charge(&mut self) -> bool {
        if self.0 == 0 {
            return false;
        }
        self.0 -= 1;
        true
    }
}

impl Default for Fuel {
    fn default() -> Self {
        Fuel::unmetered()
    }
}

#[cfg(test)]
mod tests;
