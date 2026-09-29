// audited: 2026-09-29
//! One level of fiber resumption: the swap in and out, and what it funds.
//!
//! The abort that injects an error at the suspension point instead is here too.
//!
//! docs/impl/region/park.md

use super::*;

impl VM {
    /// Execute a single fiber resume: swap fibers, run, swap back.
    ///
    /// This is the non-trampolined core. It performs one level of
    /// fiber execution. If the child calls `fiber/resume` internally,
    /// it returns SIG_SWITCH (not recursing).
    pub(super) fn do_fiber_resume_single(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        // ── Native iterator fast path ────────────────────────────────
        // Native iter fibers hold a Vec<Value> + cursor. Each resume
        // yields the next element; exhaustion kills the fiber.
        let is_native = child_handle.with(|f| f.native_iter.is_some());
        if is_native {
            return child_handle.with_mut(|f| {
                let iter = f.native_iter.as_mut().unwrap();
                if iter.cursor < iter.elements.len() {
                    let val = iter.elements[iter.cursor];
                    iter.cursor += 1;
                    f.status = FiberStatus::Paused;
                    f.signal = Some((SIG_YIELD, val));
                    (SIG_YIELD, val)
                } else {
                    // Exhaustion is terminal — the (NIL) result region is 0,
                    // so no park-retain is needed (see `incref_signal_region`).
                    f.status = FiberStatus::Dead;
                    f.signal = Some((SIG_OK, Value::NIL));
                    (SIG_OK, Value::NIL)
                }
            });
        }

        // Extract resume value and status before taking the fiber
        let (resume_value, is_first_resume, child_params_seeded, unfunded) =
            child_handle.with_mut(|child| {
                let rv = child.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
                // The resume consumes the parked signal, and with it the park's
                // funding: the payload the mint record named is gone from the
                // slot, and a later park of a different shape starts from
                // nothing (the delivery funnel's take).
                let unfunded = child.delivery.take_resume_funding();
                let first = child.status == FiberStatus::New;
                (rv, first, !child.param_frames.is_empty(), unfunded)
            });

        // Fund the crossing into a frame parked at a call that never produced
        // its result: a suspending PRIMITIVE call, or a raising call a restart
        // answers. The replayed frame re-enters at that call's continuation and
        // runs the call's compiler-emitted result release; a bytecode callee
        // funds that release with its `Return` mint, but a primitive that
        // suspends never returns and a raise never completes, so the resume
        // value stands in for the result. Without the retain the continuation
        // consumes a reference the resumer still owns, and the value is freed
        // under every holder that outlives the resume (docs/impl/region/park.md
        // § "A delivery into a replayed frame carries one owning reference";
        // `tests/elle/region-primitive-resume-uaf.lisp`,
        // `tests/elle/region-fiber-restart-uaf.lisp`). `region_of` no-ops an
        // immediate.
        if unfunded {
            let heap = unsafe { &mut *self.heap_ptr };
            let r = crate::value::arena::region_of(heap, resume_value);
            crate::value::arena::incref_for_escape(
                heap,
                r,
                crate::value::arena::EscapeSite::ResumeDelivery,
            );
        }

        // Inherit parent's parameter bindings on first resume.
        // Flatten all frames into a single frame so the child starts
        // with the parent's current dynamic bindings as its baseline.
        // Skip children already seeded by `seed_child_inheritance`: a
        // trampolined nested resume seeds from the TRUE parent before
        // switching, while `self.fiber` here is the root fiber driving
        // the trampoline — overwriting would install the wrong baseline.
        if is_first_resume && !child_params_seeded && !self.fiber.param_frames.is_empty() {
            let flat = super::flatten_param_frames(&self.fiber.param_frames);
            #[cfg(debug_assertions)]
            let borrows = super::record_param_borrows(&flat, self.heap());
            // The seeded baseline is a counted holder (docs/impl/region/park.md
            // § "A child's inherited parameter baseline is a counted holder"):
            // retain each heap entry and record the fiber → value edge; the
            // fiber object's free releases them through the baseline walk.
            super::retain_param_baseline(unsafe { &mut *self.heap_ptr }, child_value, &flat);
            child_handle.with_mut(|c| {
                c.param_frames = vec![flat];
                c.param_baseline_seeded = true;
                #[cfg(debug_assertions)]
                {
                    c.param_borrows = borrows;
                }
            });
        }

        // ── Direct fiber resumption optimization ───────────────────
        //
        // For subsequent resumes whose first suspended frame is FiberResume,
        // the full swap protocol (take/wire/swap/execute/status/extract/
        // swap-back/put) is wasted: resume_suspended would immediately set
        // pending_fiber_resume and return SIG_SWITCH without executing any
        // bytecode. Short-circuit: consume the FiberResume frame, wire the
        // inner fiber's signal, set pending_fiber_resume, and return
        // SIG_SWITCH directly. The trampoline in do_fiber_resume handles
        // the rest identically. Chains naturally through N levels.
        if !is_first_resume {
            let skip = child_handle.with(|c| {
                c.suspended.as_ref().is_some_and(|frames| {
                    matches!(frames.first(), Some(SuspendedFrame::FiberResume { .. }))
                })
            });

            if skip {
                let (inner_handle, inner_fv, remaining) = child_handle.with_mut(|c| {
                    let mut frames = c.suspended.take().unwrap();
                    let first = frames.remove(0);
                    match first {
                        SuspendedFrame::FiberResume {
                            handle,
                            fiber_value,
                        } => {
                            let rest = if frames.is_empty() {
                                None
                            } else {
                                Some(frames)
                            };
                            (handle, fiber_value, rest)
                        }
                        _ => unreachable!(),
                    }
                });

                // Preserve remaining frames for the unwind-resume path.
                child_handle.with_mut(|c| c.suspended = remaining);

                // Propagate withheld capabilities: parent → intermediate → inner.
                let intermediate_withheld = child_handle.with_mut(|c| {
                    c.withheld |= self.fiber.withheld;
                    c.withheld
                });
                inner_handle.with_mut(|f| f.withheld |= intermediate_withheld);

                // Deliver the resume value to the inner fiber (matches what
                // resume_suspended does at the FiberResume arm). The install
                // displaces any parked signal, so the payload-named funding
                // records leave with it: a park whose payload the RUNTIME built
                // is owed the release its body has none for, exactly as at
                // `fiber/resume` itself — this is the route a `protect`ed
                // body's denial takes, the denial parking in the inner fiber
                // the outer one awaits through a `FiberResume` frame
                // (docs/impl/region/park.md) — and
                // the displace clears the mint record. The resume funding
                // survives for the delivery funnel that runs when the
                // trampoline descends.
                crate::vm::fiber::release_displaced_denial_payload(self.heap(), &inner_handle);
                inner_handle.with_mut(|f| {
                    f.signal = Some((SIG_OK, resume_value));
                    f.delivery.displace();
                });

                // Set up the trampoline to descend into the inner fiber.
                // Its true parent is the intermediate fiber whose FiberResume
                // frame we just consumed (this resume's child).
                self.pending_fiber_resume = Some(super::super::core::PendingFiberResume {
                    handle: inner_handle,
                    fiber_value: inner_fv,
                    parent: Some((child_handle.clone(), Some(child_value))),
                });

                return (SIG_SWITCH, Value::NIL);
            }
        }

        // Before running the child's body, confirm every uncounted cross-fiber
        // param-snapshot borrow it inherited is still live (debug builds). A
        // borrowed region freed since the seed would be a stale read inside the
        // body; panic at the boundary, naming the parameter, instead. The seeded
        // baseline is the live cross-fiber set at this point (inner `parameterize`
        // frames are pushed only once the body runs).
        #[cfg(debug_assertions)]
        {
            let borrows = child_handle.with(|c| c.param_borrows.clone());
            if let Some((pid, r)) = super::first_stale_borrow(&borrows, self.heap()) {
                panic!(
                    "stale param-snapshot borrow on resume: parameter {pid} holds a \
                     value in region {r}, which was freed since this fiber inherited \
                     it — an uncounted cross-fiber borrow outlived its region \
                     (docs/impl/region/generations.md § 'Uncounted-borrow check')"
                );
            }
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
    pub(super) fn do_fiber_first_resume(&mut self, resume_value: Value) -> SignalBits {
        let closure = self.fiber.closure.clone();

        // Build args from resume_value based on closure arity.
        // fiber/resume provides at most one value, so we pass it as a
        // single argument when the closure expects parameters.
        let args: &[Value] = match closure.template.arity() {
            crate::value::Arity::Exact(0) => &[],
            _ => &[resume_value],
        };

        if !self.check_arity(&closure.template.arity(), args.len()) {
            return SIG_ERROR;
        }

        let env_rc = match self.build_closure_env(&closure, args) {
            Some(env) => env,
            None => {
                // Error already set on fiber.signal
                return SIG_ERROR;
            }
        };

        // Hand the body its executing-closure register via the one-shot — the
        // fiber's first resume is an entry into the body of the closure the
        // fiber was created from, so a self-recursive fiber body resolves its
        // self-reference to that closure.
        self.pending_entry_closure = self.fiber.closure_value;
        // This entrant PARKS the body's frame on an error exit (below) so the
        // restarts system can replay it, which replays the releases among its
        // remaining instructions — so the abandoned-frame walk must not run them
        // (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
        // it still owes").
        self.pending_error_park = true;
        let result = self.execute_bytecode_saving_stack(&closure.template.code(), &env_rc);

        // If the fiber signaled (not normal completion), save context for resumption.
        // Only save if the yield instruction didn't already set up suspended frames.
        // SIG_HALT is non-resumable — no suspended frame needed.
        //
        // Use the active bytecode/constants/env from ExecResult, not the
        // original closure fields — a tail call may have switched to a
        // different function's bytecode before the signal occurred.
        if !result.bits.is_empty()
            && !result.bits.intersects(SIG_HALT)
            && self.fiber.suspended.is_none()
        {
            // Use the captured inner stack so that on resume the instruction at
            // result.ip sees the same operand stack it had when it paused. This is
            // essential for SIG_FUEL: charge_fuel fires before any stack reads, so
            // args are still present and must be restored.
            //
            // push_resume_value — SIG_FUEL: re-execute the paused instruction from
            // scratch (args on the stack, nothing extra to push). A raise with no
            // result position: continue past it, the resume value going nowhere.
            // All other signals (SIG_ERROR, user-defined, etc.): the instruction at
            // result.ip expects the signal's "return value" on the stack (e.g.
            // Return needs a value to pop), so push it.
            // What the body's activation owed rode out of the popped activation in
            // `result.activation_dues` (moved, beside the region map) —
            // park it so the resumed body's completion frees it.
            let push_resume_value = !result.bits.intersects(SIG_FUEL) && result.site.delivers();
            // This exit built an error park, so it records what a restart owes
            // (docs/impl/region/park.md § "A restart delivers into an error
            // park"). A denial of `:error` in call position parks its own frame
            // and never reaches here; in tail position it does, and the ledger
            // keeps the denial's funding for its payload.
            if result.bits.intersects(SIG_ERROR) {
                let payload = self.fiber.signal.map_or(Value::NIL, |(_, v)| v);
                self.fiber.delivery.park_error(result.site, payload);
            }
            if !result.site.parks_frame() {
                self.fiber.suspended = Some(Vec::new());
                return result.bits;
            }
            let frame = BytecodeFrame::suspend(
                result.code,
                result.env,
                result.ip,
                result.stack,
                push_resume_value,
                result.activation_region_map,
                result.activation_dues,
                result.current_closure,
                self.heap(),
            );
            self.fiber.suspended = Some(vec![SuspendedFrame::Bytecode(frame)]);
        }

        result.bits
    }
    /// Resume a Suspended fiber — continue from suspended frames.
    pub(super) fn do_fiber_subsequent_resume(&mut self, resume_value: Value) -> SignalBits {
        let frames = match self.fiber.suspended.take() {
            Some(frames) => frames,
            None => {
                self.set_error(
                    "internal-error",
                    "fiber/resume: suspended fiber has no saved context",
                );
                return SIG_ERROR;
            }
        };

        self.replay_at_boundary(frames, resume_value)
    }

    /// Replay this fiber's parked chain, and record the error park the replay
    /// built, if it built one, in the fiber's delivery ledger. The one route
    /// every replay at a fiber boundary takes, so none of them can forget the
    /// park a restart will deliver into.
    fn replay_at_boundary(
        &mut self,
        frames: Vec<SuspendedFrame>,
        resume_value: Value,
    ) -> SignalBits {
        let replay = self.replay_suspended(frames, resume_value);
        if let Some(site) = replay.error_park {
            let payload = self.fiber.signal.map_or(Value::NIL, |(_, v)| v);
            self.fiber.delivery.park_error(site, payload);
        }
        replay.bits
    }
    /// Execute a fiber abort: raise the injected error at the fiber's
    /// suspension point.
    ///
    /// For a `FiberResume` park — a `protect`/`defer` sub-fiber blocked on
    /// I/O — the inner fiber is aborted recursively, so the handler that
    /// awaits it sees the error. A handler that suspends again leaves the
    /// chain parked and propagates the suspension; otherwise the replay of the
    /// frames past the sub-fiber delivers its result.
    ///
    /// For a `Bytecode` park the fiber is stopped in its own code, where
    /// nothing in it can catch: the error is raised in place, and the chain
    /// stays parked for a restart, which answers the call the fiber waits on
    /// (docs/impl/region/park.md § "A restart delivers into an error park").
    ///
    /// A fiber that never started takes the error as its value.
    pub(super) fn do_fiber_abort(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let error_value = child_handle
            .with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v))
            .unwrap_or(Value::NIL);

        let (bits, value) = self.with_child_fiber(child_handle, child_value, |vm| {
            vm.fiber.status = FiberStatus::Alive;
            // Clear the signal — prim_fiber_abort pre-set it with the error
            // value, which we already extracted above. Each arm below installs
            // what the fiber stops on.
            vm.fiber.signal = None;

            let mut frames = match vm.fiber.suspended.take() {
                Some(frames) => frames,
                None => {
                    // A fiber that never started: nothing parked takes the
                    // error, and no resume value is owed. The injection minted
                    // the payload's delivery (`AbortDelivery`), so it is
                    // recorded the way a raise records its own.
                    vm.fiber.delivery.install_abort(error_value);
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    return SIG_ERROR;
                }
            };

            // Check the innermost frame. FiberResume means a protect/defer
            // child is blocked on I/O — abort it recursively so protect
            // sees the error. Bytecode means the fiber itself is suspended
            // — raise the error there.
            match frames.first() {
                Some(SuspendedFrame::FiberResume {
                    handle,
                    fiber_value,
                }) => {
                    // The injection minted the payload's delivery
                    // (`AbortDelivery`), so record it the way a raise records its
                    // own: with the delivery funded independently, a frame of
                    // THIS fiber that owns a reference to the payload funds
                    // nothing, and the abandoned-frame walk and the parked
                    // frame's discharge must stop exempting the payload's region
                    // (docs/impl/region/mechanism.md § "An abandoned frame runs
                    // the releases it still owes"). The replay below delivers the
                    // inner fiber's result, funded by that same mint rather than
                    // by a resume mint (docs/impl/region/park.md § "A delivery
                    // into a replayed frame carries one owning reference").
                    vm.fiber.delivery.install_abort(error_value);
                    let inner_handle = handle.clone();
                    let inner_value = *fiber_value;

                    // Abort the inner fiber (e.g. protect child blocked on I/O).
                    // Store the error on the inner fiber so do_fiber_abort picks it up.
                    // The install displaces the inner fiber's park, so a payload
                    // the RUNTIME built there is owed its release first — the
                    // `protect`ed face of the injection's own (see
                    // `inject_error_at_suspension`).
                    crate::vm::fiber::release_displaced_denial_payload(vm.heap(), &inner_handle);
                    inner_handle.with_mut(|f| {
                        f.signal = Some((SIG_ERROR, error_value));
                    });
                    let (inner_bits, inner_result) = vm.do_fiber_abort(&inner_handle, inner_value);

                    // The inner fiber's handlers are ordinary code, so they can
                    // suspend again — a `protect` body that continues into an
                    // I/O call after it captures the injected error, a `defer`
                    // cleanup that writes to a port. The inner fiber then still
                    // owes its continuation, and this fiber's continuation must
                    // not run ahead of it: park the chain as it stands and
                    // propagate, exactly as the trampoline's unwind does for the
                    // same signal on a plain resume. The resume re-enters the
                    // inner fiber first, and only its completion delivers the
                    // value the frames below wait for.
                    //
                    // `mask_catches`, not `VM::absorbs`: this is a lookahead at
                    // what the INNER fiber's mask will do, and absorbs nothing
                    // itself, so an error still travelling must keep the
                    // location recorded for it.
                    let inner_mask = inner_handle.with(|f| f.mask);
                    if !super::catch::mask_catches(inner_mask, inner_bits)
                        && !super::is_terminal_signal(inner_bits)
                    {
                        vm.fiber.signal = Some((inner_bits, inner_result));
                        vm.fiber.suspended = Some(frames);
                        return inner_bits;
                    }

                    // Resume remaining frames so protect/defer cleanup runs.
                    let remaining: Vec<SuspendedFrame> = frames.drain(1..).collect();
                    if remaining.is_empty() {
                        vm.fiber.signal = Some((inner_bits, inner_result));
                        inner_bits
                    } else {
                        // The replayed frame's pending release consumes one
                        // owning reference of the value it is resumed with (the
                        // parked call's compiler-emitted result release). A
                        // normally-completing child funds that reference with
                        // its Return's ReturnValue retain; the aborted child's
                        // ERROR exit runs no Return, and the injection's
                        // `AbortDelivery` retain stands in — this replay is one
                        // of the four consumers that mint answers for, and it
                        // takes no retain of its own. Pinned by
                        // `region_fiber_abort_io_protect_uaf`;
                        // tests/elle/grpc.lisp is the full-scheduler witness.
                        vm.replay_at_boundary(remaining, inner_result)
                    }
                }
                Some(SuspendedFrame::Bytecode(frame)) => {
                    // Raise in place: the fiber stops on the error at the call
                    // it waits on, with its whole chain still parked, so a
                    // restart answers that call and the frames outside it run
                    // on. The park keeps the funding it was built with — a
                    // primitive or denial park still owes the resume mint, an
                    // emit park or a fuel pause does not — and the injection's
                    // mint is recorded with the payload.
                    vm.record_error_loc(frame.code.locations(), frame.ip);
                    vm.fiber.delivery.raise_in_park(error_value);
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    vm.fiber.suspended = Some(frames);
                    SIG_ERROR
                }
                None => {
                    // An empty chain: take the error as the value, as for a
                    // fiber that never started.
                    vm.fiber.delivery.install_abort(error_value);
                    vm.fiber.signal = Some((SIG_ERROR, error_value));
                    SIG_ERROR
                }
            }
        });

        // The abort's replay runs cleanup code (defer/protect) that may itself
        // call fiber/resume; under the trampoline that surfaces as SIG_SWITCH +
        // pending_fiber_resume rather than recursing. Drive it to a real signal
        // — a no-op for every other result.
        self.finish_fiber_resume(bits, value, child_handle, child_value)
    }
}
