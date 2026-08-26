//! `CheckSignalBound` opcode body.
//!
//! Split out of the dispatch match: formatting a `restrict` violation from the
//! global registry is verbose enough to crowd the routing.

use super::*;

impl VM {
    /// `CheckSignalBound`: verify the closure on the stack cannot emit any
    /// signal outside `allowed_bits`.
    ///
    /// Non-closure operands carry no signal metadata and silently pass — only
    /// closures are checked. A violation sets `SIG_ERROR` with the excess and
    /// allowed signals formatted from the global registry.
    #[inline]
    pub(super) fn handle_check_signal_bound(&mut self, bc: &[u8], ip: &mut usize) {
        let allowed_bits = self.read_signal_bits(bc, ip);
        let val = self.fiber.stack.pop().unwrap_or(Value::NIL);
        if let Some(closure) = val.as_closure() {
            let signal_bits = closure.signal().bits;
            let excess = signal_bits.subtract(allowed_bits);
            if !excess.is_empty() {
                let excess_str = crate::signals::registry::format_bits(excess);
                let allowed_str = crate::signals::registry::format_bits(allowed_bits);
                let err = self.escaping_error(
                    "signal-violation",
                    format!(
                        "restrict: closure may emit {} but parameter is restricted to {}",
                        excess_str, allowed_str
                    ),
                );
                self.fiber.signal = Some((SIG_ERROR, err));
            }
        }
        // Non-closure values (primitives, etc.) are silent — they pass any
        // signal bound check. Only closures carry signal metadata.
    }
}
