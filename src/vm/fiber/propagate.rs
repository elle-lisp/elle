//! `fiber/propagate`: re-raise a child fiber's caught signal to this fiber's
//! own parent. Call- and tail-position handlers (see the `super` module doc).

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};
use crate::vm::core::VM;

impl VM {
    /// Handle SIG_PROPAGATE from fiber/propagate (Call position).
    pub(in crate::vm) fn handle_fiber_propagate_signal(
        &mut self,
        fiber_value: Value,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_PROPAGATE with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (child_bits, child_value) = handle.with(|fiber| fiber.signal).unwrap_or_else(|| {
            (
                SIG_ERROR,
                self.escaping_error("internal-error", "fiber/propagate: no signal"),
            )
        });

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.contains(SIG_ERROR) || child_bits.contains(SIG_HALT) {
            self.fiber.stack.push(Value::NIL);
            None
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            self.set_error(
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            self.fiber.stack.push(Value::NIL);
            None
        } else {
            Some(child_bits)
        }
    }

    /// Handle SIG_PROPAGATE from fiber/propagate (TailCall position).
    pub(in crate::vm) fn handle_fiber_propagate_signal_tail(
        &mut self,
        fiber_value: Value,
    ) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_PROPAGATE with non-fiber value");
                return SIG_ERROR;
            }
        };

        let (child_bits, child_value) = handle.with(|fiber| fiber.signal).unwrap_or_else(|| {
            (
                SIG_ERROR,
                self.escaping_error("internal-error", "fiber/propagate: no signal"),
            )
        });

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.contains(SIG_ERROR) || child_bits.contains(SIG_HALT) {
            child_bits
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            self.set_error(
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            SIG_ERROR
        } else {
            child_bits
        }
    }
}
