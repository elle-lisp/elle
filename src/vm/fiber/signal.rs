//! The interpreter-level SIG_RESUME handlers (Call and TailCall position) and
//! the trampoline entry `do_fiber_resume`, plus the child-inheritance seeding
//! and dead-fiber finalization they lean on. At the ROOT these drive the child
//! directly; INSIDE a fiber they suspend a continuation and hand the child to
//! the driving trampoline (see the `super` module doc for the swap protocol).

use super::*;

impl VM {
    /// Promote a fiber to `:dead` and free everything it owns — the halt-promotion
    /// twin of `with_child_fiber`'s completion arm (docs/impl/region/owner.md
    /// § "Owner nodes" — "Fiber teardown frees everything the fiber owns"). Dead is
    /// terminal (`fiber/resume` refuses it), so the parked chain and owner nodes can
    /// never be replayed. An `:error` promotion must NOT come here — an errored
    /// fiber is resumable (the restarts system) and keeps its parked state.
    pub(crate) fn finalize_dead_fiber(&mut self, handle: &FiberHandle) {
        let owned = handle.with_mut(|fiber| {
            fiber.status = FiberStatus::Dead;
            take_fiber_owned(fiber)
        });
        release_fiber_owned(unsafe { &mut *self.heap_ptr }, owned);
    }

    /// Inheritance a trampolined resume must propagate eagerly, while the
    /// calling fiber is still the active one. `do_fiber_resume_single` will
    /// run for the child from the ROOT fiber's context (the trampoline in
    /// `do_fiber_resume`), so anything it derives from `self.fiber` there
    /// would come from the wrong fiber:
    /// - a New child's dynamic parameter baseline (mirrors the inheritance
    ///   in `do_fiber_resume_single`, which skips already-seeded children);
    /// - withheld capabilities (monotonic OR, so `with_child_fiber`'s later
    ///   parent→child OR is harmless on top).
    fn seed_child_inheritance(&mut self, handle: &FiberHandle, fiber_value: Value) {
        // Skip a child already seeded at CREATION (`fiber/new` snapshots the
        // creator's parameter bindings into `param_frames`): overwriting it here
        // with THIS fiber's current frames would install the resumer's bindings
        // instead of the creator's — the exact ev/spawn bug creation-time capture
        // fixes. Seed only a still-unseeded New child, from a non-empty baseline.
        let (child_is_new, child_unseeded) =
            handle.with(|f| (f.status == FiberStatus::New, f.param_frames.is_empty()));
        if child_is_new && child_unseeded && !self.fiber.param_frames.is_empty() {
            let flat = flatten_param_frames(&self.fiber.param_frames);
            #[cfg(debug_assertions)]
            let borrows = record_param_borrows(&flat, self.heap());
            handle.with_mut(|c| {
                c.param_frames = vec![flat];
                #[cfg(debug_assertions)]
                {
                    c.param_borrows = borrows;
                }
            });
        }
        let withheld = self.fiber.withheld;
        handle.with_mut(|f| f.withheld |= withheld);
        // Child-chain wiring: while we are suspended awaiting this child,
        // `fiber/child` of this fiber must report it (recursion parity —
        // with_child_fiber would have set this on us had we recursed).
        self.fiber.child = Some(handle.clone());
        self.fiber.child_value = Some(fiber_value);
    }

    // ── SIG_RESUME: fiber execution ───────────────────────────────

    /// Handle SIG_RESUME from a fiber primitive (Call position).
    ///
    /// At the ROOT (no enclosing fiber): calls `do_fiber_resume`, whose
    /// trampoline drives arbitrarily deep nesting iteratively.
    ///
    /// INSIDE a fiber (current_fiber_handle is Some): recursing into
    /// `do_fiber_resume` here would add Rust stack frames per nesting level
    /// and overflow the host stack at depth. Instead the calling fiber
    /// suspends with its continuation frame, hands the child to the driving
    /// trampoline via `pending_fiber_resume`, and returns SIG_SWITCH. The
    /// trampoline descends iteratively; on the child's completion its unwind
    /// loop resumes our continuation frame with the result pushed.
    // `pub(in crate::vm)`: originally `pub(super)` in `fiber.rs` (super = `vm`);
    // moving the method one level deeper preserves the same effective visibility,
    // as `crate::vm::signal` is the caller.
    pub(in crate::vm) fn handle_fiber_resume_signal(
        &mut self,
        fiber_value: Value,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_RESUME with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        if self.current_fiber_handle.is_some() {
            self.seed_child_inheritance(&handle, fiber_value);
            let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
            let activation_region_map = self
                .fiber
                .activation_region_maps
                .last()
                .cloned()
                .unwrap_or_default();
            // MOVE the caller's owner node into its continuation park — this
            // activation unwinds with the SIG_SWITCH handoff
            // (docs/impl/region/owner.md § "Owner nodes").
            let activation_owner_node = self.take_activation_owner_node();
            // The closure that called `fiber/resume` is the one to resume into.
            let current_closure = self.fiber.current_closure;
            let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                code.clone(),
                closure_env.clone(),
                *ip,
                caller_stack,
                true,
                activation_region_map,
                activation_owner_node,
                current_closure,
                self.heap(),
            ));
            self.fiber.suspended = Some(vec![caller_frame]);
            let parent = self
                .current_fiber_handle
                .clone()
                .map(|h| (h, self.current_fiber_value));
            self.pending_fiber_resume = Some(super::super::core::PendingFiberResume {
                handle,
                fiber_value,
                parent,
            });
            self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
            return Some(SIG_SWITCH);
        }

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            self.finalize_dead_fiber(&handle);
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

            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) || result_bits.contains(SIG_HALT) {
                    self.fiber.stack.push(Value::NIL);
                    None
                } else {
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    let activation_region_map = self
                        .fiber
                        .activation_region_maps
                        .last()
                        .cloned()
                        .unwrap_or_default();
                    // MOVE the caller's owner node into its continuation park —
                    // this activation unwinds with the child's suspending
                    // signal (docs/impl/region/owner.md § "Owner nodes").
                    let activation_owner_node = self.take_activation_owner_node();
                    // Caller activation's remap (the resumed sub-fiber runs
                    // on its own frame stack; this frame continues us).
                    let current_closure = self.fiber.current_closure;
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                        code.clone(),
                        closure_env.clone(),
                        *ip,
                        caller_stack,
                        true,
                        activation_region_map,
                        activation_owner_node,
                        current_closure,
                        self.heap(),
                    ));
                    self.fiber.suspended = Some(vec![fiber_resume_frame, caller_frame]);
                    Some(result_bits)
                }
            }
        }
    }

    /// Handle SIG_RESUME from a fiber primitive (TailCall position).
    ///
    /// Same trampoline split as the Call-position variant. In tail position the
    /// remaining continuation is the post-`TailCall` block — the ReturnValue
    /// retain, the compiler's owned-arg releases, and the `Return` that hands
    /// the child's result up — which the standard interrupted-frame park
    /// captures and the resume replays (see the in-body comment).
    // `pub(in crate::vm)`: see `handle_fiber_resume_signal` — same widening from
    // the original `pub(super)` in `fiber.rs` to keep `crate::vm::signal` reach.
    pub(in crate::vm) fn handle_fiber_resume_signal_tail(
        &mut self,
        fiber_value: Value,
    ) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_RESUME with non-fiber value");
                return SIG_ERROR;
            }
        };

        if self.current_fiber_handle.is_some() {
            self.seed_child_inheritance(&handle, fiber_value);
            // `suspended` is deliberately left untouched (None): the standard
            // interrupted-frame parks (`do_fiber_first_resume`,
            // `resume_suspended`'s re-suspend, `call_inner`) then capture this
            // frame's continuation at the post-`TailCall` ip, and the resume
            // replays it — running the compiler's owned-arg releases exactly as
            // a non-suspending native tail call falls through to them
            // (docs/impl/region/owner.md § "Park/unpark symmetry"). Parking an
            // empty chain here instead would complete this fiber directly with
            // the child's result, stranding every owned tail arg's moved-in
            // reference (one region per nested drained fiber — the
            // `fiber-nested` oracle probe).
            let parent = self
                .current_fiber_handle
                .clone()
                .map(|h| (h, self.current_fiber_value));
            self.pending_fiber_resume = Some(super::super::core::PendingFiberResume {
                handle,
                fiber_value,
                parent,
            });
            self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
            return SIG_SWITCH;
        }

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            self.finalize_dead_fiber(&handle);
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

            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
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
    ///
    /// `pub(crate)` (not `pub(super)`): the fiber-lifecycle runtime tests drive a
    /// child fiber through park, resume, and completion with this entry.
    pub(crate) fn do_fiber_resume(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let (bits, value) = self.do_fiber_resume_single(child_handle, child_value);
        self.finish_fiber_resume(bits, value, child_handle, child_value)
    }
}
