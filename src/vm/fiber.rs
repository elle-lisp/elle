//! Fiber execution: resume, propagate, cancel.
//!
//! All fiber operations follow the same swap protocol:
//! 1. Take child fiber out of its handle
//! 2. Wire parent/child chain (Janet semantics)
//! 3. Swap parent out, child in
//! 4. Execute the child
//! 5. Set provisional status (Dead or Suspended)
//! 6. Extract result
//! 7. Swap back
//! 8. Put child back into its handle
//!
//! Status finalization happens in the caller, not in `with_child_fiber`:
//! - Resume: SIG_ERROR + uncaught by mask → Error (terminal)
//! - Resume: SIG_ERROR + caught by mask → Suspended (resumable)
//! - Cancel: always Error (terminal), regardless of mask

use crate::value::error_val;
use crate::value::fiber::FiberStatus;
use crate::value::{FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_OK};
use std::rc::Rc;

use super::core::VM;

/// Helper: set an error signal on a fiber.
fn set_error(fiber: &mut crate::value::Fiber, kind: &str, msg: impl Into<String>) {
    fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
}

impl VM {
    // ── Shared swap protocol ────────────────────────────────────────

    /// Execute a closure with the child fiber swapped in as the active fiber.
    ///
    /// Handles the full swap protocol: take child from handle, wire
    /// parent/child chain, swap fibers, run the closure, update status,
    /// extract result, swap back, put child back.
    ///
    /// `child_value` is the NaN-boxed Value wrapping the child's FiberHandle,
    /// cached on the parent so `fiber/child` can return it without re-allocating.
    ///
    /// Returns `(signal_bits, signal_value)` from the child's execution.
    fn with_child_fiber(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
        execute: impl FnOnce(&mut VM) -> SignalBits,
    ) -> (SignalBits, Value) {
        // 1. Take child fiber out of its handle (sets slot to None)
        let mut child_fiber = child_handle.take();

        // 2. Wire up parent/child chain (Janet semantics)
        self.fiber.child = Some(child_handle.clone());
        self.fiber.child_value = Some(child_value);
        child_fiber.parent = self.current_fiber_handle.as_ref().map(|h| h.downgrade());
        child_fiber.parent_value = self.current_fiber_value;

        // 3. Swap parent out, child in; track the child's handle and value
        let parent_handle = self.current_fiber_handle.take();
        let parent_value = self.current_fiber_value.take();
        self.current_fiber_handle = Some(child_handle.clone());
        self.current_fiber_value = Some(child_value);
        std::mem::swap(&mut self.fiber, &mut child_fiber);

        // 4. Execute the closure
        let bits = execute(self);

        // 5. Update child status based on result.
        //    SIG_OK is terminal (Dead). Other signals are Suspended; the
        //    caller decides whether a caught SIG_ERROR stays Suspended
        //    (resumable) or gets promoted to Error (terminal) based on
        //    the parent's mask.
        self.fiber.status = if bits == SIG_OK {
            FiberStatus::Dead
        } else {
            FiberStatus::Suspended
        };

        // 6. Extract the result before swapping back
        let result_value = self
            .fiber
            .signal
            .as_ref()
            .map(|(_, v)| *v)
            .unwrap_or(Value::NIL);
        let result_bits = self.fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(bits);

        // 7. Swap back: parent in, child out; restore parent's handle and value
        std::mem::swap(&mut self.fiber, &mut child_fiber);
        self.current_fiber_handle = parent_handle;
        self.current_fiber_value = parent_value;

        // 8. Put child fiber back into its handle
        child_handle.put(child_fiber);

        (result_bits, result_value)
    }

    // ── SIG_RESUME: fiber execution ───────────────────────────────

    /// Handle SIG_RESUME from a fiber primitive (Call position).
    pub(super) fn handle_fiber_resume_signal(
        &mut self,
        fiber_value: Value,
        _bytecode: &Rc<Vec<u8>>,
        _constants: &Rc<Vec<Value>>,
        _closure_env: &Rc<Vec<Value>>,
        _ip: &mut usize,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(&mut self.fiber, "error", "SIG_RESUME with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);

        // Check the child's signal against its mask
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits == SIG_OK {
            // Child completed normally — clear child chain
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else if mask & result_bits != 0 {
            // Signal is caught by the mask — parent handles it, clear child chain
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            // Signal is NOT caught — propagate to parent.
            // Leave parent.child set (preserves trace chain for stack traces).
            // Mark child as terminally errored (not resumable).
            if result_bits == SIG_ERROR {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.signal = Some((result_bits, result_value));
            if result_bits == SIG_ERROR {
                self.fiber.stack.push(Value::NIL);
                None // dispatch loop will see the error signal
            } else {
                Some(result_bits)
            }
        }
    }

    /// Handle SIG_RESUME from a fiber primitive (TailCall position).
    pub(super) fn handle_fiber_resume_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(&mut self.fiber, "error", "SIG_RESUME with non-fiber value");
                return SIG_ERROR;
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);

        let caught = result_bits == SIG_OK || (mask & result_bits != 0);
        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            // Uncaught SIG_ERROR → terminal error status on child
            if result_bits == SIG_ERROR {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            self.fiber.signal = Some((result_bits, result_value));
            result_bits
        }
    }

    /// Execute a fiber resume: swap fibers, run, swap back.
    fn do_fiber_resume(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        // Extract resume value and status before taking the fiber
        let (resume_value, is_first_resume) = child_handle.with_mut(|child| {
            let rv = child.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
            let first = child.status == FiberStatus::New;
            (rv, first)
        });

        self.with_child_fiber(child_handle, child_value, |vm| {
            vm.fiber.status = FiberStatus::Alive;

            if is_first_resume {
                vm.do_fiber_first_resume()
            } else {
                vm.do_fiber_subsequent_resume(resume_value)
            }
        })
    }

    /// First resume of a New fiber — build env and execute closure bytecode.
    ///
    /// Uses execute_bytecode_saving_stack (not execute_bytecode_inner) because
    /// the fiber body may end with a TailCall. execute_bytecode_saving_stack
    /// handles pending tail calls in a loop, while execute_bytecode_inner does
    /// not.
    fn do_fiber_first_resume(&mut self) -> SignalBits {
        let closure = self.fiber.closure.clone();
        let env_rc = self.build_closure_env(&closure, &[]);

        let (bits, ip) =
            self.execute_bytecode_saving_stack(&closure.bytecode, &closure.constants, &env_rc);

        // If the fiber signaled (not normal completion), save context for resumption.
        // Only save if the yield instruction didn't already set up suspended frames.
        if bits != SIG_OK && self.fiber.suspended.is_none() {
            self.fiber.suspended = Some(vec![SuspendedFrame {
                bytecode: closure.bytecode.clone(),
                constants: closure.constants.clone(),
                env: env_rc,
                ip,
                stack: vec![],
            }]);
        }

        bits
    }

    /// Resume a Suspended fiber — continue from suspended frames.
    fn do_fiber_subsequent_resume(&mut self, resume_value: Value) -> SignalBits {
        let frames = match self.fiber.suspended.take() {
            Some(frames) => frames,
            None => {
                set_error(
                    &mut self.fiber,
                    "error",
                    "fiber/resume: suspended fiber has no saved context",
                );
                return SIG_ERROR;
            }
        };

        self.resume_suspended(frames, resume_value)
    }

    // ── SIG_PROPAGATE: re-raise caught signal ─────────────────────

    /// Handle SIG_PROPAGATE from fiber/propagate (Call position).
    pub(super) fn handle_fiber_propagate_signal(
        &mut self,
        fiber_value: Value,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "error",
                    "SIG_PROPAGATE with non-fiber value",
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (child_bits, child_value) = handle
            .with(|fiber| fiber.signal)
            .unwrap_or((SIG_ERROR, error_val("error", "fiber/propagate: no signal")));

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits == SIG_ERROR {
            self.fiber.stack.push(Value::NIL);
            None
        } else {
            Some(child_bits)
        }
    }

    /// Handle SIG_PROPAGATE from fiber/propagate (TailCall position).
    pub(super) fn handle_fiber_propagate_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "error",
                    "SIG_PROPAGATE with non-fiber value",
                );
                return SIG_ERROR;
            }
        };

        let (child_bits, child_value) = handle
            .with(|fiber| fiber.signal)
            .unwrap_or((SIG_ERROR, error_val("error", "fiber/propagate: no signal")));

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));
        child_bits
    }

    // ── SIG_CANCEL: inject error into fiber ───────────────────────

    /// Handle SIG_CANCEL from fiber/cancel (Call position).
    pub(super) fn handle_fiber_cancel_signal(
        &mut self,
        fiber_value: Value,
        _bytecode: &Rc<Vec<u8>>,
        _constants: &Rc<Vec<Value>>,
        _closure_env: &Rc<Vec<Value>>,
        _ip: &mut usize,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(&mut self.fiber, "error", "SIG_CANCEL with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (result_bits, result_value) = self.do_fiber_cancel(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);
        let caught = result_bits == SIG_OK || (mask & result_bits != 0);

        // Cancelled fibers are always terminal regardless of mask
        handle.with_mut(|f| f.status = FiberStatus::Error);

        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            self.fiber.signal = Some((result_bits, result_value));
            if result_bits == SIG_ERROR {
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                Some(result_bits)
            }
        }
    }

    /// Handle SIG_CANCEL from fiber/cancel (TailCall position).
    pub(super) fn handle_fiber_cancel_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(&mut self.fiber, "error", "SIG_CANCEL with non-fiber value");
                return SIG_ERROR;
            }
        };

        let (result_bits, result_value) = self.do_fiber_cancel(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);
        let caught = result_bits == SIG_OK || (mask & result_bits != 0);

        // Cancelled fibers are always terminal regardless of mask
        handle.with_mut(|f| f.status = FiberStatus::Error);
        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            self.fiber.signal = Some((result_bits, result_value));
            result_bits
        }
    }

    /// Execute a fiber cancel: inject error, resume, let error handlers run.
    fn do_fiber_cancel(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let error_value = child_handle
            .with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v))
            .unwrap_or(Value::NIL);

        self.with_child_fiber(child_handle, child_value, |vm| {
            // Inject the error signal
            vm.fiber.status = FiberStatus::Alive;
            vm.fiber.signal = Some((SIG_ERROR, error_value));

            // Resume the fiber so error handlers can run
            if let Some(frames) = vm.fiber.suspended.take() {
                // Resume from suspended frames — the error signal is already set,
                // so the dispatch loop will see it on the first instruction check
                vm.resume_suspended(frames, Value::NIL)
            } else {
                // New fiber that was never started — just mark as errored
                SIG_ERROR
            }
        })
    }
}
