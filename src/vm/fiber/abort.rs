//! `fiber/abort`: inject an error into a fiber and resume it for unwinding. The
//! child's outcome (dead/error/paused) decides what the parent sees — no status
//! stomp. Call- and tail-position handlers (see the `super` module doc).

use std::rc::Rc;

use crate::value::fiber::FiberStatus;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};
use crate::vm::core::VM;

impl VM {
    /// Park an abort's outcome as this fiber's OWN propagating signal — the
    /// uncaught arm, shared by all three positions so they cannot drift.
    ///
    /// The payload's delivery is funded before it gets here, and never by this
    /// frame: the injection minted it (`AbortDelivery`) where the fiber unwound
    /// with the value it was aborted with, and the child's own raise minted it
    /// where the fiber raised an error of its own instead. So a slot of this
    /// frame that holds the payload owes a release like any other, and the
    /// record is what stops the abandoned-frame walk exempting it
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A materialized literal handed straight to `fiber/abort` is
    /// the shape that reaches this — it lives in a frame slot and in nothing
    /// else (the `abort-discard` probe in `tests/elle/oracle.lisp`).
    pub(in crate::vm::fiber) fn park_propagating_abort(&mut self, bits: SignalBits, value: Value) {
        self.fiber.signal = Some((bits, value));
        if bits.intersects(SIG_ERROR) {
            self.fiber.delivery.record_mint(value);
        }
    }

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

        if self.absorbs(&handle, mask, result_bits, result_value) {
            // Abort is terminal — even if the parent catches the signal,
            // the aborted fiber is finished and must not stay :paused.
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            // Uncaught error → terminal
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.reject_orphaned_signal(result_bits, "fiber/abort") {
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                self.park_propagating_abort(result_bits, result_value);
                if result_bits.intersects(SIG_ERROR) {
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

        if self.absorbs(&handle, mask, result_bits, result_value) {
            // Abort is terminal — set child to :error even when caught
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.reject_orphaned_signal(result_bits, "fiber/abort") {
                SIG_ERROR
            } else {
                self.park_propagating_abort(result_bits, result_value);
                result_bits
            }
        }
    }
}
