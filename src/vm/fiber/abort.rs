//! `fiber/abort`: inject an error into a fiber and resume it for unwinding. The
//! child's outcome (dead/error/paused) decides what the parent sees — no status
//! stomp. Call- and tail-position handlers (see the `super` module doc).

use std::rc::Rc;

use crate::value::fiber::FiberStatus;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};
use crate::vm::core::VM;
use crate::vm::fiber::mask_catches;

impl VM {
    /// Mint the caller's reference to an aborted child's ERROR result — the one
    /// the missing `Return` would have taken.
    ///
    /// A caught abort hands `result_value` to the caller as the call's result, so
    /// the caller's `DecrefValueRegion` fires on it. A normally-completing child
    /// funds that release with its `Return`'s `IncrefValueRegion`; an unwinding
    /// child runs no `Return` at all, and its payload is in general a value the
    /// caller already owns and already releases — its own `fiber/abort` argument.
    /// Without this mint the two releases run against one reference and the
    /// payload is freed under the fiber that still parks it
    /// (`region_fiber_abort_delivery_uaf`).
    ///
    /// Only the caught arm mints: the uncaught arm pushes `nil` and routes the
    /// payload through the signal instead, where no caller release targets it.
    /// The park-retain the child's own `signal` hold owes is separate and taken by
    /// `with_child_fiber` (docs/impl/region/effects.md § `Delivers`).
    ///
    /// All three positions that drive an abort — call, tail, and JIT — share this,
    /// so they cannot drift apart on the accounting.
    pub(in crate::vm::fiber) fn mint_abort_error_result(&mut self, result_value: Value) {
        let heap = unsafe { &mut *self.heap_ptr };
        let r = crate::value::arena::region_of(heap, result_value);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::ReturnValue,
        );
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

        if mask_catches(mask, result_bits) {
            // Abort is terminal — even if the parent catches the signal,
            // the aborted fiber is finished and must not stay :paused.
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
                self.mint_abort_error_result(result_value);
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
                self.fiber.signal = Some((result_bits, result_value));
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

        if mask_catches(mask, result_bits) {
            // Abort is terminal — set child to :error even when caught
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
                self.mint_abort_error_result(result_value);
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
                self.fiber.signal = Some((result_bits, result_value));
                result_bits
            }
        }
    }
}
