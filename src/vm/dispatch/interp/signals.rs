// audited: 2026-10-04
//! `CheckSignalBound` opcode body: the entry check of a `(silence p)` bound,
//! shared with the JIT through `signals::bound`.
//!
//! docs/impl/vm.md

use super::*;

impl VM {
    /// `CheckSignalBound`: verify the value on the stack cannot emit any
    /// signal outside `allowed_bits`.
    ///
    /// `signals::bound::violation` decides, for this tier and the JIT alike: a
    /// closure by its effective signal, a native by its declared one, and any
    /// other value passes. A violation sets `SIG_ERROR` with the excess and
    /// allowed signals formatted from the global registry.
    #[inline]
    pub(super) fn handle_check_signal_bound(&mut self, bc: &[u8], ip: &mut usize) {
        let allowed_bits = self.read_signal_bits(bc, ip);
        let val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CheckSignalBound");
        if let Some(message) = crate::signals::bound::violation(val, allowed_bits) {
            let err = self.escaping_error("signal-violation", message);
            self.fiber.signal = Some((SIG_ERROR, err));
        }
    }
}

#[cfg(test)]
mod tests;
