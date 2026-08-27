use super::*;

impl VM {
    /// Handle SIG_RESUME from a fiber primitive in JIT context.
    #[cfg(feature = "jit")]
    ///
    /// Runs the child fiber synchronously and returns the result as `JitValue`.
    /// On error: sets fiber.signal, returns `JitValue::nil()`.
    /// On yield propagation: sets fiber.signal, returns YIELD_SENTINEL.
    pub(crate) fn handle_fiber_resume_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_RESUME with non-fiber value");
                return JitValue::nil();
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);

        self.finalize_if_halted(&handle, result_bits);

        if self.absorbs(&handle, mask, result_bits, result_value) {
            self.fiber.child = None;
            self.fiber.child_value = None;
            JitValue::from_value(result_value)
        } else {
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.reject_orphaned_signal(result_bits, "fiber/resume") {
                JitValue::nil()
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.intersects(SIG_ERROR) || result_bits.intersects(SIG_HALT) {
                    JitValue::nil()
                } else {
                    // Uncaught non-error signal (yield, I/O, etc.) — side-exit.
                    // Create a FiberResume frame so that resume_suspended will
                    // re-resume the child fiber after the signal is resolved.
                    // Without this, the raw io-request value leaks through as
                    // the call result instead of the resolved I/O value.
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let mut frames = self.fiber.suspended.take().unwrap_or_default();
                    frames.push(fiber_resume_frame);
                    self.fiber.suspended = Some(frames);
                    YIELD_SENTINEL
                }
            }
        }
    }
    /// Handle SIG_PROPAGATE from fiber/propagate in JIT context.
    #[cfg(feature = "jit")]
    pub(crate) fn handle_fiber_propagate_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_PROPAGATE with non-fiber value");
                return JitValue::nil();
            }
        };

        // Same install as the bytecode handlers, so the same delivery reference
        // is owed — routed through the one helper rather than a third copy
        // (docs/impl/region/owner.md § "Park/unpark symmetry").
        let (child_bits, child_value) = self.take_propagated_signal(&handle);

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.intersects(SIG_ERROR) || child_bits.intersects(SIG_HALT) {
            JitValue::nil()
        } else if self.current_fiber_handle.is_none() {
            self.set_error(
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            JitValue::nil()
        } else {
            YIELD_SENTINEL
        }
    }
    /// Handle SIG_ABORT from fiber/abort in JIT context.
    #[cfg(feature = "jit")]
    pub(crate) fn handle_fiber_abort_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_ABORT with non-fiber value");
                return JitValue::nil();
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
            JitValue::from_value(result_value)
        } else {
            if result_bits.intersects(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.reject_orphaned_signal(result_bits, "fiber/abort") {
                JitValue::nil()
            } else {
                self.park_propagating_abort(result_bits, result_value);
                if result_bits.intersects(SIG_ERROR) || result_bits.intersects(SIG_HALT) {
                    JitValue::nil()
                } else {
                    YIELD_SENTINEL
                }
            }
        }
    }
}
