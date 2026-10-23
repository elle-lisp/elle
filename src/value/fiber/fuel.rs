// audited: 2026-10-22
//! `Fuel`: a fiber's instruction budget, the charge each charging instruction
//! takes from it, and the kill word every charge reads first.
//!
//! docs/runtime.md
//! docs/impl/jit.md
//!
//! The budget is one 64-bit word, and an unmetered fiber holds a word above
//! every budget, so a charge is a load, a test for zero and a store on either
//! tier, and a silent function's spend is a load and a store. The kill word is
//! a second word a thread handle shares with the VM it can kill, and a budget
//! below it ends the fiber. Compiled code reads both through the VM pointer;
//! [`Fuel::WORD_OFFSET_IN_VM`] and [`Fuel::KILL_PTR_OFFSET_IN_VM`] are where
//! it finds them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A fiber's instruction budget: a count of units left, or no budget at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Fuel(u64);

/// The kill word a VM's charge sites read, shared with every thread handle
/// that can kill the VM (docs/runtime.md). A worker's switch is made by the
/// spawning thread and handed to the worker, so a pull before the worker's
/// first charge lands at that charge.
#[derive(Clone, Debug, Default)]
pub struct KillSwitch(Arc<AtomicU64>);

impl KillSwitch {
    /// The word of a VM nobody killed: no budget is below it.
    pub const SLACK: u64 = 0;
    /// The word of a killed VM: every budget is below it, the unmetered word
    /// included.
    pub const PULLED: u64 = u64::MAX;

    /// A slack switch of its own.
    pub fn new() -> Self {
        KillSwitch(Arc::new(AtomicU64::new(Self::SLACK)))
    }

    /// Kill the VM that reads this switch, at its next charge.
    pub fn pull(&self) {
        self.0.store(Self::PULLED, Ordering::Relaxed);
    }

    /// Whether the switch was pulled.
    pub fn pulled(&self) -> bool {
        self.limit() == Self::PULLED
    }

    /// The word as a charge reads it. A relaxed load: the pull carries no
    /// other data, and a charge that reads the word a trip late lands the kill
    /// one trip late.
    #[inline]
    pub(crate) fn limit(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Where the word lives, for compiled code to read through the VM.
    pub(crate) fn word(&self) -> *const AtomicU64 {
        Arc::as_ptr(&self.0)
    }
}

impl Fuel {
    /// The largest budget: the largest integer a program can write. Any word
    /// above it is a fiber with no budget.
    const LARGEST: u64 = i64::MAX as u64;

    /// The word of a fiber with no budget: above every budget, below a pulled
    /// switch, and 2^62 decrements from the budget range (docs/impl/jit.md).
    const UNMETERED: u64 = (1 << 63) + (1 << 62);

    /// Whether a charge against the kill word `limit` kills the fiber instead.
    #[inline]
    pub fn is_below(self, limit: u64) -> bool {
        self.0 < limit
    }

    /// The byte offset of the budget word from the start of a `VM`, which is
    /// what compiled code charges through its VM pointer.
    #[cfg(feature = "jit")]
    pub(crate) const WORD_OFFSET_IN_VM: usize =
        std::mem::offset_of!(crate::vm::VM, fiber) + std::mem::offset_of!(super::Fiber, fuel);

    /// The byte offset from the start of a `VM` of the pointer to its kill
    /// word, which compiled code loads before each charge.
    #[cfg(feature = "jit")]
    pub(crate) const KILL_PTR_OFFSET_IN_VM: usize = std::mem::offset_of!(crate::vm::VM, kill_word);

    /// No budget: the fiber runs until it finishes or suspends.
    pub const fn unmetered() -> Self {
        Fuel(Self::UNMETERED)
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
