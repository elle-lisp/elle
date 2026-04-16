//! Fiber execution: resume, propagate, abort, cancel.
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
//! - Abort: inject error + resume, result handled like resume (no stomp)
//! - Cancel: hard kill — set status to Error, drop frames, no resume
//!
//! SIG_TERMINAL signals are uncatchable — they pass through mask checks.

use crate::jit::JitValue;
use crate::value::error_val;
use crate::value::fiber::FiberStatus;
use crate::value::{
    BytecodeFrame, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_FUEL, SIG_HALT,
    SIG_OK, SIG_SWITCH, SIG_TERMINAL,
};
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
    /// `child_value` is the cached Value wrapping the child's FiberHandle,
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

        // 2a. Propagate withheld capabilities: child inherits parent's withheld.
        // This is idempotent (OR is monotonic) so safe on repeated resume.
        child_fiber.withheld |= self.fiber.withheld;

        // 3. Swap parent out, child in; track the child's handle and value
        let parent_handle = self.current_fiber_handle.take();
        let parent_value = self.current_fiber_value.take();
        self.current_fiber_handle = Some(child_handle.clone());
        self.current_fiber_value = Some(child_value);
        std::mem::swap(&mut self.fiber, &mut child_fiber);

        // 3a. Install child's fiber heap as the active allocation target.
        //     Save whatever was active (parent's heap, always non-null after issue-525).
        let saved_heap = crate::value::fiberheap::save_current_heap();
        unsafe {
            crate::value::fiberheap::install_fiber_heap(
                &mut *self.fiber.heap as *mut crate::value::FiberHeap,
            );
        }
        // 3b. Install outbox for yield-bound allocations. The compiler
        // emits OutboxEnter/OutboxExit around yield/emit value expressions,
        // routing those allocations to the outbox. The parent reads
        // yielded values directly from the outbox (zero-copy).
        //
        // Default allocation target is the child's private heap. Only
        // allocations between OutboxEnter/OutboxExit go to the outbox.
        // install_outbox tears down the previous outbox (reset-on-resume).
        let tmpl = &self.fiber.closure.template;
        if !tmpl.result_is_immediate || tmpl.signal.may_suspend() || tmpl.has_outward_heap_set {
            self.fiber
                .heap
                .install_outbox(crate::value::fiberheap::pool::SlabPool::new());
        }

        // 4. Execute the closure
        let bits = execute(self);

        // 5. Update child status based on result.
        //    SIG_OK is terminal (Dead). Other signals are Suspended; the
        //    caller decides whether a caught SIG_ERROR stays Suspended
        //    (resumable) or gets promoted to Error (terminal) based on
        //    the parent's mask. SIG_HALT is also provisionally Suspended
        //    here — the resume handler promotes to Dead if the mask
        //    doesn't catch it.
        self.fiber.status = if bits.is_ok() {
            FiberStatus::Dead
        } else {
            FiberStatus::Paused
        };

        // 6. Extract the result before swapping back.
        //    Safety net: if the value is heap-allocated in the child's
        //    private pool (not in the outbox), deep-copy to the outbox
        //    so the parent doesn't read a dangling pointer.
        let mut result_value = self
            .fiber
            .signal
            .as_ref()
            .map(|(_, v)| *v)
            .unwrap_or(Value::NIL);
        let result_bits = self.fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(bits);

        if result_value.is_heap()
            && self.fiber.heap.has_outbox()
            && self.fiber.heap.value_in_private_pool(result_value)
        {
            result_value = self.fiber.heap.deep_copy_to_outbox(result_value);
            // Update the signal with the new value so the parent reads the copy.
            if let Some(ref mut sig) = self.fiber.signal {
                sig.1 = result_value;
            }
        }

        // 7. Swap back: parent in, child out; restore parent's heap and handle
        unsafe {
            crate::value::fiberheap::restore_saved_heap(saved_heap);
        }
        std::mem::swap(&mut self.fiber, &mut child_fiber);
        self.current_fiber_handle = parent_handle;
        self.current_fiber_value = parent_value;

        // 8. Put child fiber back into its handle
        child_handle.put(child_fiber);

        (result_bits, result_value)
    }

    // ── SIG_RESUME: fiber execution ───────────────────────────────

    /// Handle SIG_RESUME from a fiber primitive (Call position).
    /// Handle SIG_RESUME from a fiber primitive (Call position).
    ///
    /// Calls do_fiber_resume directly. The trampoline inside do_fiber_resume
    /// handles nested fiber/resume iteratively (via the FiberResume frame
    /// path in resume_suspended returning SIG_SWITCH).
    ///
    /// TODO(trampoline): This should return SIG_SWITCH instead of calling
    /// do_fiber_resume, to unwind the Rust call stack. See height.md and
    /// the plan at .claude/plans/resilient-percolating-bubble.md.
    /// The blocker: when SIG_SWITCH propagates out of execute_bytecode_from_ip
    /// (inside resume_suspended) after a TailCall changed the bytecode,
    /// the frame chain in resume_suspended's Bytecode arm loses the
    /// continuation context because exec.stack is empty (vec![]).
    /// Fix: make execute_bytecode_from_ip capture the inner stack on
    /// non-OK exit (like execute_bytecode_saving_stack does), then use
    /// that stack in the exec frame that resume_suspended builds.
    pub(super) fn handle_fiber_resume_signal(
        &mut self,
        fiber_value: Value,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        location_map: &Rc<crate::error::LocationMap>,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_RESUME with non-fiber value",
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            handle.with_mut(|f| f.status = FiberStatus::Dead);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) {
                    self.fiber.stack.push(Value::NIL);
                    None
                } else {
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
                        bytecode: bytecode.clone(),
                        constants: constants.clone(),
                        env: closure_env.clone(),
                        ip: *ip,
                        stack: caller_stack,
                        location_map: location_map.clone(),
                        push_resume_value: true,
                    });
                    self.fiber.suspended = Some(vec![fiber_resume_frame, caller_frame]);
                    Some(result_bits)
                }
            }
        }
    }

    /// Handle SIG_RESUME from a fiber primitive (TailCall position).
    pub(super) fn handle_fiber_resume_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_RESUME with non-fiber value",
                );
                return SIG_ERROR;
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            handle.with_mut(|f| f.status = FiberStatus::Dead);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                SIG_ERROR
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if !result_bits.contains(SIG_ERROR) && !result_bits.contains(SIG_HALT) {
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let mut existing = self.fiber.suspended.take().unwrap_or_default();
                    let mut all = vec![fiber_resume_frame];
                    all.append(&mut existing);
                    self.fiber.suspended = Some(all);
                }
                result_bits
            }
        }
    }

    /// Execute a fiber resume with trampoline for nested fiber/resume.
    ///
    /// If the child fiber itself calls `fiber/resume` (setting
    /// `pending_fiber_resume` and returning SIG_SWITCH), we iterate
    /// rather than recursing on the Rust call stack.
    pub(super) fn do_fiber_resume(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let (mut bits, mut value) = self.do_fiber_resume_single(child_handle, child_value);

        // Fast path: no nested fiber/resume — return directly.
        if bits != SIG_SWITCH {
            return (bits, value);
        }

        // Slow path: trampoline for nested fiber/resume.
        //
        // fiber_stack records fibers that suspended because they called
        // fiber/resume on a deeper fiber. We iterate instead of recursing.
        let mut fiber_stack: Vec<(FiberHandle, Value)> = vec![];
        fiber_stack.push((child_handle.clone(), child_value));

        loop {
            if bits == SIG_SWITCH {
                // A fiber called fiber/resume on a child: descend.
                let pending = self
                    .pending_fiber_resume
                    .take()
                    .expect("VM bug: SIG_SWITCH without pending_fiber_resume");

                fiber_stack.push((pending.handle.clone(), pending.fiber_value));
                let (new_bits, new_value) =
                    self.do_fiber_resume_single(&pending.handle, pending.fiber_value);
                bits = new_bits;
                value = new_value;
                continue;
            }

            // Real signal from the current deepest fiber. Unwind the stack.
            loop {
                let (current_handle, current_fv) = fiber_stack.pop().unwrap();
                let mask = current_handle.with(|f| f.mask);

                if bits.contains(SIG_HALT) {
                    current_handle.with_mut(|f| f.status = FiberStatus::Dead);
                }

                let caught = bits.is_ok() || (mask.covers(bits) && !bits.contains(SIG_TERMINAL));

                if caught {
                    self.fiber.child = None;
                    self.fiber.child_value = None;

                    // Update the fiber's signal so fiber/value returns
                    // the correct result to Elle code.
                    current_handle.with_mut(|f| {
                        f.signal = Some((bits, value));
                    });

                    if fiber_stack.is_empty() {
                        // Back to the original caller.
                        return (bits, value);
                    }

                    // Resume the parent fiber: it was suspended waiting
                    // for this child to complete.
                    let (parent_handle, parent_fv) = fiber_stack.last().unwrap();
                    parent_handle.with_mut(|f| {
                        f.signal = Some((SIG_OK, value));
                    });

                    let (new_bits, new_value) =
                        self.do_fiber_resume_single(parent_handle, *parent_fv);
                    bits = new_bits;
                    value = new_value;
                    // Break to outer loop to check for SIG_SWITCH.
                    break;
                } else {
                    // Signal NOT caught — propagate through fiber stack.
                    if bits.contains(SIG_ERROR) {
                        current_handle.with_mut(|f| f.status = FiberStatus::Error);
                    }

                    if fiber_stack.is_empty() {
                        return (bits, value);
                    }

                    // For uncaught suspending signals (e.g. SIG_IO), build
                    // FiberResume frame on the parent so the suspension chain
                    // preserves the fiber nesting for re-entry.
                    if !bits.contains(SIG_ERROR) && !bits.contains(SIG_HALT) {
                        let (parent_handle, _) = fiber_stack.last().unwrap();
                        let child_resume_frame = SuspendedFrame::FiberResume {
                            handle: current_handle.clone(),
                            fiber_value: current_fv,
                        };
                        parent_handle.with_mut(|f| {
                            let mut new_frames = vec![child_resume_frame];
                            if let Some(mut existing) = f.suspended.take() {
                                new_frames.append(&mut existing);
                            }
                            f.suspended = Some(new_frames);
                            f.signal = Some((bits, value));
                        });
                    }
                    // Continue unwinding to the next parent.
                }
            }
        }
    }

    /// Execute a single fiber resume: swap fibers, run, swap back.
    ///
    /// This is the non-trampolined core. It performs one level of
    /// fiber execution. If the child calls `fiber/resume` internally,
    /// it returns SIG_SWITCH (not recursing).
    fn do_fiber_resume_single(
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

        // Inherit parent's parameter bindings on first resume.
        // Flatten all frames into a single frame so the child starts
        // with the parent's current dynamic bindings as its baseline.
        if is_first_resume && !self.fiber.param_frames.is_empty() {
            let mut flat: Vec<(u32, Value)> = Vec::new();
            for frame in &self.fiber.param_frames {
                for &(id, val) in frame {
                    if let Some(pos) = flat.iter().position(|&(k, _)| k == id) {
                        flat[pos].1 = val;
                    } else {
                        flat.push((id, val));
                    }
                }
            }
            child_handle.with_mut(|c| c.param_frames = vec![flat]);
        }

        self.with_child_fiber(child_handle, child_value, |vm| {
            vm.fiber.status = FiberStatus::Alive;

            if is_first_resume {
                vm.do_fiber_first_resume(resume_value)
            } else {
                vm.do_fiber_subsequent_resume(resume_value)
            }
        })
    }

    /// First resume of a New fiber — build env and execute closure bytecode.
    ///
    /// The `resume_value` is passed as the closure's argument when the
    /// closure expects a parameter (e.g., a signal parameter). For
    /// zero-parameter closures, no arguments are passed.
    ///
    /// Uses execute_bytecode_saving_stack (not execute_bytecode_inner) because
    /// the fiber body may end with a TailCall. execute_bytecode_saving_stack
    /// handles pending tail calls in a loop, while execute_bytecode_inner does
    /// not.
    fn do_fiber_first_resume(&mut self, resume_value: Value) -> SignalBits {
        let closure = self.fiber.closure.clone();

        // Build args from resume_value based on closure arity.
        // fiber/resume provides at most one value, so we pass it as a
        // single argument when the closure expects parameters.
        let args: &[Value] = match closure.template.arity {
            crate::value::Arity::Exact(0) => &[],
            _ => &[resume_value],
        };

        if !self.check_arity(&closure.template.arity, args.len()) {
            return SIG_ERROR;
        }

        let env_rc = match self.build_closure_env(&closure, args) {
            Some(env) => env,
            None => {
                // Error already set on fiber.signal
                return SIG_ERROR;
            }
        };

        let result = self.execute_bytecode_saving_stack(
            &closure.template.bytecode,
            &closure.template.constants,
            &env_rc,
            &closure.template.location_map,
        );

        // If the fiber signaled (not normal completion), save context for resumption.
        // Only save if the yield instruction didn't already set up suspended frames.
        // SIG_HALT is non-resumable — no suspended frame needed.
        //
        // Use the active bytecode/constants/env from ExecResult, not the
        // original closure fields — a tail call may have switched to a
        // different function's bytecode before the signal occurred.
        if !result.bits.is_ok() && !result.bits.contains(SIG_HALT) && self.fiber.suspended.is_none()
        {
            self.fiber.suspended = Some(vec![SuspendedFrame::Bytecode(BytecodeFrame {
                bytecode: result.bytecode,
                constants: result.constants,
                env: result.env,
                ip: result.ip,
                // Use the captured inner stack so that on resume the instruction
                // at result.ip sees the same operand stack it had when it paused.
                // This is essential for SIG_FUEL: check_fuel! fires before any
                // stack reads, so args are still present and must be restored.
                stack: result.stack,
                location_map: result.location_map,
                // SIG_FUEL: re-execute the paused instruction from scratch —
                // args are on the stack, nothing extra to push.
                // All other signals (SIG_ERROR, user-defined, etc.): the
                // instruction at result.ip expects the signal's "return value"
                // on the stack (e.g. Return needs a value to pop). Push it.
                push_resume_value: !result.bits.contains(SIG_FUEL),
            })]);
        }

        result.bits
    }

    /// Resume a Suspended fiber — continue from suspended frames.
    fn do_fiber_subsequent_resume(&mut self, resume_value: Value) -> SignalBits {
        let frames = match self.fiber.suspended.take() {
            Some(frames) => frames,
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "fiber/resume: suspended fiber has no saved context",
                );
                return SIG_ERROR;
            }
        };

        self.resume_suspended(frames, resume_value)
    }

    // ── SIG_PROPAGATE: propagate caught signal ────────────────────

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
                    "internal-error",
                    "SIG_PROPAGATE with non-fiber value",
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        let (child_bits, child_value) = handle.with(|fiber| fiber.signal).unwrap_or((
            SIG_ERROR,
            error_val("internal-error", "fiber/propagate: no signal"),
        ));

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.contains(SIG_ERROR) {
            self.fiber.stack.push(Value::NIL);
            None
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            set_error(
                &mut self.fiber,
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
    pub(super) fn handle_fiber_propagate_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_PROPAGATE with non-fiber value",
                );
                return SIG_ERROR;
            }
        };

        let (child_bits, child_value) = handle.with(|fiber| fiber.signal).unwrap_or((
            SIG_ERROR,
            error_val("internal-error", "fiber/propagate: no signal"),
        ));

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.contains(SIG_ERROR) {
            child_bits
        } else if self.current_fiber_handle.is_none() {
            // At root fiber: no parent to catch the propagated signal
            set_error(
                &mut self.fiber,
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            SIG_ERROR
        } else {
            child_bits
        }
    }

    // ── SIG_ABORT: inject error into fiber, resume for unwinding ──

    /// Handle SIG_ABORT from fiber/abort (Call position).
    ///
    /// Injects an error and resumes the fiber. The result is handled
    /// identically to fiber/resume — the child's actual outcome (dead,
    /// error, paused) determines what the parent sees. No status stomp.
    pub(super) fn handle_fiber_abort_signal(
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
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_ABORT with non-fiber value",
                );
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
            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
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
    pub(super) fn handle_fiber_abort_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_ABORT with non-fiber value",
                );
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
            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
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

    // ── JIT-context fiber signal handlers ────────────────────────────
    //
    // These mirror the interpreter-level handlers above but return `JitValue`
    // instead of pushing to fiber.stack. Called from jit/dispatch.rs when a
    // primitive returns SIG_RESUME/SIG_PROPAGATE/SIG_ABORT in JIT context.

    /// Handle SIG_RESUME from a fiber primitive in JIT context.
    ///
    /// Runs the child fiber synchronously and returns the result as `JitValue`.
    /// On error: sets fiber.signal, returns `JitValue::nil()`.
    /// On yield propagation: sets fiber.signal, returns YIELD_SENTINEL.
    pub(crate) fn handle_fiber_resume_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_RESUME with non-fiber value",
                );
                return JitValue::nil();
            }
        };

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);

        let mask = handle.with(|fiber| fiber.mask);

        if result_bits == SIG_HALT {
            handle.with_mut(|f| f.status = FiberStatus::Dead);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            self.fiber.child = None;
            self.fiber.child_value = None;
            JitValue::from_value(result_value)
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                JitValue::nil()
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) {
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
    pub(crate) fn handle_fiber_propagate_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_PROPAGATE with non-fiber value",
                );
                return JitValue::nil();
            }
        };

        let (child_bits, child_value) = handle.with(|fiber| fiber.signal).unwrap_or((
            SIG_ERROR,
            error_val("internal-error", "fiber/propagate: no signal"),
        ));

        self.fiber.child = Some(handle);
        self.fiber.child_value = Some(fiber_value);
        self.fiber.signal = Some((child_bits, child_value));

        if child_bits.contains(SIG_ERROR) {
            JitValue::nil()
        } else if self.current_fiber_handle.is_none() {
            set_error(
                &mut self.fiber,
                "state-error",
                "fiber/propagate: cannot propagate signal (no parent fiber to catch it)",
            );
            JitValue::nil()
        } else {
            YIELD_SENTINEL
        }
    }

    /// Handle SIG_ABORT from fiber/abort in JIT context.
    pub(crate) fn handle_fiber_abort_signal_jit(&mut self, fiber_value: Value) -> JitValue {
        use crate::jit::YIELD_SENTINEL;

        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                set_error(
                    &mut self.fiber,
                    "internal-error",
                    "SIG_ABORT with non-fiber value",
                );
                return JitValue::nil();
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
            JitValue::from_value(result_value)
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }
            if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
                set_error(
                    &mut self.fiber,
                    "state-error",
                    "fiber/abort: cannot propagate signal (no parent fiber to catch it)",
                );
                JitValue::nil()
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) {
                    JitValue::nil()
                } else {
                    YIELD_SENTINEL
                }
            }
        }
    }

    /// Execute a fiber abort: inject error into the fiber's execution context.
    ///
    /// For `FiberResume` frames (protect/defer children blocked on I/O),
    /// the inner fiber is aborted recursively so that protect/defer sees
    /// the child error and runs cleanup code.
    ///
    /// For `Bytecode` frames (direct bytecode suspension), the error is
    /// set on `fiber.signal` so the dispatch loop returns it immediately.
    /// The error then propagates through the caller's protect/defer chain.
    fn do_fiber_abort(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let error_value = child_handle
            .with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v))
            .unwrap_or(Value::NIL);

        self.with_child_fiber(child_handle, child_value, |vm| {
            vm.fiber.status = FiberStatus::Alive;
            // Clear the signal — prim_fiber_abort pre-set it with the error
            // value, which we already extracted above. If we leave it set,
            // the dispatch loop will see SIG_ERROR and bail immediately
            // when we try to resume remaining bytecode frames.
            vm.fiber.signal = None;

            let frames = match vm.fiber.suspended.take() {
                Some(frames) => frames,
                None => {
                    // New fiber that was never started — just mark as errored
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    return SIG_ERROR;
                }
            };

            // Check the innermost frame. FiberResume means a protect/defer
            // child is blocked on I/O — abort it recursively so protect
            // sees the error. Bytecode means the fiber itself is suspended
            // — set the error and let the dispatch loop return it.
            match frames.first() {
                Some(SuspendedFrame::FiberResume {
                    handle,
                    fiber_value,
                }) => {
                    let inner_handle = handle.clone();
                    let inner_value = *fiber_value;

                    // Abort the inner fiber (e.g. protect child blocked on I/O).
                    // Store the error on the inner fiber so do_fiber_abort picks it up.
                    inner_handle.with_mut(|f| {
                        f.signal = Some((SIG_ERROR, error_value));
                    });
                    let (inner_bits, inner_result) = vm.do_fiber_abort(&inner_handle, inner_value);

                    // Resume remaining frames so protect/defer cleanup runs.
                    let remaining: Vec<SuspendedFrame> = frames[1..].to_vec();
                    if remaining.is_empty() {
                        vm.fiber.signal = Some((inner_bits, inner_result));
                        inner_bits
                    } else {
                        vm.resume_suspended(remaining, inner_result)
                    }
                }
                Some(SuspendedFrame::Bytecode(_)) => {
                    // Innermost frame is bytecode — set error and resume
                    // through the chain. The dispatch loop will see SIG_ERROR
                    // and return immediately from this frame, then outer
                    // frames run normally (defer/protect).
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    vm.resume_suspended(frames, Value::NIL)
                }
                None => {
                    // No frames (shouldn't happen — we checked above)
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    SIG_ERROR
                }
            }
        })
    }
}
