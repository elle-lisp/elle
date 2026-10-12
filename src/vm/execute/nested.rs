// audited: 2026-10-06
//! Interpreted non-tail calls on one dispatch loop: a caller pauses in the
//! fiber while its callee runs, and resumes when the callee ends.
//!
//! docs/impl/vm.md

use super::{ExecResult, Exit};
use crate::value::fiber::{Activation, CallSite, ParamDepth, PausedCaller, TailBoundary};
use crate::value::{BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT};
use crate::vm::core::{PendingCall, VM};
use std::rc::Rc;

impl VM {
    /// Run one activation's dispatch loop from `start_ip`, with every
    /// interpreted non-tail callee it reaches run on the same loop.
    ///
    /// To its driver this is the dispatch loop itself: it answers the `Exit` at
    /// which the activation it was entered with leaves, the calls that
    /// activation made having completed in between. A caller waits
    /// in `fiber.callers` while its callee runs, so the Rust stack stays flat
    /// however deep the calls go (docs/impl/vm.md).
    pub(in crate::vm) fn run_dispatch(
        &mut self,
        code: &crate::value::Code,
        env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> Exit {
        let mut exit = self.execute_bytecode_inner_impl(code, env, start_ip);
        if self.pending_call.is_none() {
            return exit;
        }
        #[cfg(debug_assertions)]
        let base = self.fiber.callers.len();
        // The activation whose dispatch loop just exited. `None` is the one this
        // call was entered with, whose code and environment are the arguments.
        let mut running: Option<Activation> = None;
        loop {
            let Exit { bits, ip, .. } = exit;
            if let Some(call) = self.pending_call.take() {
                debug_assert!(
                    bits.is_empty(),
                    "VM bug: a call hands over its callee only on a clean exit"
                );
                let callee = self.enter_callee(running.take(), ip, call);
                exit = self.execute_bytecode_inner_impl(&callee.code, &callee.env, 0);
                running = Some(callee);
                continue;
            }
            let Some(mut callee) = running.take() else {
                #[cfg(debug_assertions)]
                debug_assert_eq!(
                    self.fiber.callers.len(),
                    base,
                    "VM bug: the entered activation exited over paused callers"
                );
                return exit;
            };
            if bits.is_empty() {
                if let Some(mut tail) = self.pending_tail_call.take() {
                    callee
                        .tail_boundary
                        .add(std::mem::take(&mut tail.boundary), self.fiber.param_depth());
                    (callee.code, callee.env) = self.replace_by_tail_call(tail);
                    exit = self.execute_bytecode_inner_impl(&callee.code, &callee.env, 0);
                    running = Some(callee);
                    continue;
                }
            }
            let (result, site) = self.leave_callee(callee, exit);
            let caller = self
                .fiber
                .callers
                .pop()
                .expect("VM bug: a callee returns into a paused caller");
            self.fiber.stack.clear();
            self.fiber.stack.extend(caller.stack);
            self.fiber.current_closure = caller.closure;
            running = caller.activation;
            let (caller_code, caller_env) = match &running {
                Some(activation) => (&activation.code, &activation.env),
                None => (code, env),
            };
            exit = match self.complete_call(
                caller_code,
                caller_env,
                caller.resume_ip,
                site,
                caller.param_depth,
                result,
            ) {
                None => self.execute_bytecode_inner_impl(caller_code, caller_env, caller.resume_ip),
                // The call instruction leaves the caller by the callee's signal,
                // as it would have left the dispatch loop.
                Some(bits) => {
                    if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) {
                        self.record_error_loc(caller_code.locations(), caller.call_ip);
                    }
                    Exit::at(bits, caller.resume_ip)
                }
            };
        }
    }

    /// Pause the running activation — `caller`, or the entered one when that
    /// is `None` — at `resume_ip`, and open the callee's activation.
    fn enter_callee(
        &mut self,
        caller: Option<Activation>,
        resume_ip: usize,
        call: PendingCall,
    ) -> Activation {
        let stack = self.fiber.stack.drain(..).collect();
        self.fiber.callers.push(PausedCaller {
            activation: caller,
            resume_ip,
            call_ip: call.call_ip,
            stack,
            param_depth: self.fiber.param_depth(),
            closure: self.fiber.current_closure,
        });
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(call.closure, &call.code);
        self.fiber.current_closure = call.closure;
        #[cfg(debug_assertions)]
        let entry_depth = self.fiber.activation_region_maps.len();
        self.open_activation();
        Activation {
            code: call.code,
            env: call.env,
            tail_boundary: TailBoundary::none(self.fiber.param_depth()),
            call: call.site,
            #[cfg(debug_assertions)]
            entry_depth,
        }
    }

    /// End a callee's activation whose dispatch loop exited at `exit`,
    /// answering how it ended and the call that entered it.
    fn leave_callee(&mut self, callee: Activation, exit: Exit) -> (ExecResult, CallSite) {
        #[cfg(debug_assertions)]
        let entry_depth = callee.entry_depth;
        let mut result = self.end_activation(callee.code, callee.env, exit, callee.tail_boundary);
        self.close_activation(
            &mut result,
            #[cfg(debug_assertions)]
            entry_depth,
        );
        (result, callee.call)
    }

    /// Complete a non-tail closure call whose callee ended with `result`, in
    /// the caller running `code` with `env`, which resumes at `resume_ip`.
    /// `depth` is the parameter depth at the call.
    ///
    /// `None`: the result is on the caller's stack and the caller continues.
    /// `Some(bits)`: the caller leaves by `bits` too. A suspend or an error has
    /// parked the caller behind the callee, and an error or halt keeps the trace
    /// frame for the stack trace.
    fn complete_call(
        &mut self,
        code: &crate::value::Code,
        env: &Rc<Vec<Value>>,
        resume_ip: usize,
        site: CallSite,
        depth: ParamDepth,
        mut result: ExecResult,
    ) -> Option<SignalBits> {
        self.fiber.call_depth -= 1;
        let bits = result.bits;
        let raised = bits.intersects(SIG_ERROR) && !bits.intersects(SIG_HALT);

        // An error parks the callee that raised it, as a suspend parked it
        // before returning here, so a boundary that refuses the signal below
        // discards the callee's frame with the rest of the chain.
        if raised {
            self.park_raised(&mut result);
        }

        // The callee's boundary may refuse its signal: a broken proof, or a
        // signal its squelch names. `enforce_boundary` discards the suspended
        // frames, so the call leaves by the refusal's error, not suspended.
        //
        // A fiber body is exempt: it runs outside any call, so its first resume
        // crosses no boundary.
        if let Some(refusal) = self.enforce_boundary(bits, &site.boundary, depth) {
            self.fiber.call_stack.pop();
            return Some(refusal);
        }
        if bits.is_empty() {
            let (_, value) = self.fiber.signal.take().unwrap();
            self.fiber.stack.push(value);
            self.fiber.call_stack.pop();
            return None;
        }
        if bits.intersects(SIG_HALT) {
            // The call frame is preserved for the stack trace.
            return Some(bits);
        }

        // Every other signal parks the caller behind the callee for resumption,
        // an error included (docs/impl/region/unwind.md). The caller frame is
        // built whether or not the callee populated fiber.suspended: a callee
        // that tail-called a native yielding primitive creates no SuspendedFrame
        // (TCO), so fiber.suspended may be None here.
        let (_, value) = self.fiber.signal.take().unwrap();
        let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
        if self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::CALL)
            && caller_stack.len() <= 5
        {
            eprintln!(
                "[call_inner suspend] ip={} bc_len={} stack_depth={}",
                resume_ip,
                code.bytecode().len(),
                caller_stack.len(),
            );
            for (si, sv) in caller_stack.iter().enumerate() {
                eprintln!("  stack[{}] = {} {:?}", si, sv.type_name(), sv);
            }
        }
        // The callee's activation is closed, so `activation_region_maps.last()`
        // is the caller's.
        let caller_region_frame = self
            .fiber
            .activation_region_maps
            .last()
            .cloned()
            .unwrap_or_default();
        // MOVE what the caller's activation owes into its park — this activation
        // leaves with the suspending signal (docs/impl/region/owner.md).
        let caller_dues = self.take_activation_dues();
        // The caller's register is restored, so park the caller's value here; the
        // callee's value rode out in `result.current_closure`.
        let caller_closure = self.fiber.current_closure;
        let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
            code.clone(),
            env.clone(),
            resume_ip,
            caller_stack,
            true,
            caller_region_frame,
            caller_dues,
            caller_closure,
            self.heap(),
        ));

        let mut frames = self.fiber.suspended.take().unwrap_or_default();
        // Preserve a non-yield-suspended (e.g. SIG_FUEL) callee's inner frame —
        // it lives in result.stack, not fiber.suspended (see
        // park_suspended_callee_frame). An error parked it above.
        if !raised {
            self.park_suspended_callee_frame(&mut frames, bits, result);
        }
        if self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::FIBER)
        {
            eprintln!(
                "[call_inner] suspend: bits={} ip={} bc_len={} inner_frames={} env_len={}",
                bits,
                resume_ip,
                code.bytecode().len(),
                frames.len(),
                env.len(),
            );
        }
        frames.push(caller_frame);
        self.fiber.signal = Some((bits, value));
        self.fiber.suspended = Some(frames);
        // An error keeps the trace frame for the stack trace.
        if !raised {
            self.fiber.call_stack.pop();
        }
        Some(bits)
    }
}
