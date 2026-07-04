//! `fiber/abort`: inject an error into a fiber and resume it for unwinding. The
//! child's outcome (dead/error/paused) decides what the parent sees — no status
//! stomp. Call- and tail-position handlers (see the `super` module doc).

use std::rc::Rc;

use crate::value::fiber::FiberStatus;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_TERMINAL};
use crate::vm::core::VM;

impl VM {
    /// Handle SIG_ABORT from fiber/abort (Call position).
    ///
    /// Injects an error and resumes the fiber. The result is handled
    /// identically to fiber/resume — the child's actual outcome (dead,
    /// error, paused) determines what the parent sees. No status stomp.
    pub(in crate::vm) fn handle_fiber_abort_signal(
        &mut self,
        fiber_value: Value,
        _code: &crate::value::Code,
        _closure_env: &Rc<Vec<Value>>,
        _ip: &mut usize,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_ABORT with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (result_bits, result_value) = self.do_fiber_abort(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.is_ok() || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL))
        {
            // Abort is terminal — even if the parent catches the signal,
            // the aborted fiber is finished and must not stay :paused.
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            // Uncaught error → terminal
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
                    "state-error",
                    "fiber/abort: cannot propagate signal (no parent fiber to catch it)",
                );
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) {
                    self.fiber.stack.push(Value::NIL);
                    None
                } else {
                    Some(result_bits)
                }
            }
        }
    }

    /// Handle SIG_ABORT from fiber/abort (TailCall position).
    pub(in crate::vm) fn handle_fiber_abort_signal_tail(
        &mut self,
        fiber_value: Value,
    ) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_ABORT with non-fiber value");
                return SIG_ERROR;
            }
        };

        let (result_bits, result_value) = self.do_fiber_abort(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            // Abort is terminal — set child to :error even when caught
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
                    "state-error",
                    "fiber/abort: cannot propagate signal (no parent fiber to catch it)",
                );
                SIG_ERROR
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                result_bits
            }
        }
    }
}
