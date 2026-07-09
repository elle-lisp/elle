//! `CheckSignalBound` opcode body.
//!
//! Split out of the dispatch match: decoding a four-word `SignalBits` mask and
//! formatting a `restrict` violation is verbose enough to crowd the routing.
//! Behavior is unchanged.

use super::*;

impl VM {
    /// `CheckSignalBound`: verify the closure on the stack cannot emit any
    /// signal outside `allowed_bits`.
    ///
    /// The mask is encoded as four `u16` words (least-significant first).
    /// Non-closure operands carry no signal metadata and silently pass — only
    /// closures are checked. A violation sets `SIG_ERROR` with the excess and
    /// allowed signals formatted from the global registry.
    #[inline]
    pub(super) fn handle_check_signal_bound(&mut self, bc: &[u8], ip: &mut usize) {
        // Read SignalBits as four u16s (least-significant first)
        let w0 = self.read_u16(bc, ip) as u64;
        let w1 = self.read_u16(bc, ip) as u64;
        let w2 = self.read_u16(bc, ip) as u64;
        let w3 = self.read_u16(bc, ip) as u64;
        let allowed_bits = SignalBits::new(w0 | (w1 << 16) | (w2 << 32) | (w3 << 48));
        let val = self.fiber.stack.pop().unwrap_or(Value::NIL);
        if let Some(closure) = val.as_closure() {
            let signal_bits = closure.signal().bits;
            let excess = signal_bits.subtract(allowed_bits);
            if !excess.is_empty() {
                let registry = crate::signals::registry::global_registry().lock().unwrap();
                let excess_str = registry.format_signal_bits(excess);
                let allowed_str = registry.format_signal_bits(allowed_bits);
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
