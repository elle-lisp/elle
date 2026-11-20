// audited: 2026-10-21
//! The expansion budget: one count every expansion on a VM spends, apart from fiber fuel.
//!
//! docs/runtime.md
//! docs/macros.md

use crate::value::fiber::Fuel;
use crate::value::{SignalBits, SIG_HALT};
use crate::vm::core::VM;

/// The fuel an expansion may spend, with every expansion nested inside it,
/// whatever fuel its fiber holds.
pub const EXPANSION_FUEL: u64 = 1 << 24;

/// The error kind of the halt a spent expansion budget raises.
pub(super) const SPENT_KIND: &str = "out-of-fuel";

/// The budget of the expansion running on a VM, apart from the fuel of every
/// fiber. Unmetered while no expansion runs, so a charge then takes nothing.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ExpansionBudget(Fuel);

impl ExpansionBudget {
    /// Whether an expansion is running.
    #[inline]
    pub(crate) fn is_live(self) -> bool {
        self.0.is_metered()
    }

    /// Take one unit. Answers false, and takes nothing, when an expansion runs
    /// and its budget is spent: the charging instruction must then halt it.
    #[inline]
    pub(crate) fn charge(&mut self) -> bool {
        !self.0.is_metered() || self.0.charge()
    }

    /// Start the budget, unless an expansion already runs. Answers whether this
    /// call started it, which makes the caller the one to [`Self::close`] it.
    pub(super) fn open(&mut self) -> bool {
        if self.is_live() {
            return false;
        }
        self.0 = Fuel::budget(EXPANSION_FUEL).expect("the expansion budget is an integer");
        true
    }

    /// End the budget: no expansion runs.
    pub(super) fn close(&mut self) {
        self.0 = Fuel::unmetered();
    }

    /// The units left, or `None` while no expansion runs.
    #[cfg(test)]
    pub(super) fn remaining(self) -> Option<u64> {
        self.0.remaining()
    }
}

impl VM {
    /// Halt the running expansion, whose budget a charge at `resume_ip` found
    /// spent. A halt, not an error, because no handler can give the budget
    /// back, so it passes every mask and silent boundary on its way to
    /// `at_expansion`.
    pub(crate) fn halt_spent_expansion(&mut self, resume_ip: usize) -> (SignalBits, usize) {
        let err = self.escaping_error(
            SPENT_KIND,
            format!("the expansion fuel budget of {EXPANSION_FUEL} units is spent"),
        );
        self.fiber.signal = Some((SIG_HALT, err));
        (SIG_HALT, resume_ip)
    }

    /// Whether code runs inside an expansion, where every call runs on the
    /// interpreter so each charge site spends the budget.
    #[inline]
    pub(crate) fn expanding(&self) -> bool {
        self.expansion_budget.is_live()
    }
}
