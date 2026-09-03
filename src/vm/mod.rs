pub mod arithmetic;
pub mod call;
pub mod capture;
// Note: jit_entry is not pub — it only adds impl VM methods
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod dispatch;
pub mod env;
pub mod eval;
pub mod execute;
pub mod fiber;
#[cfg(feature = "jit")]
mod jit_entry;
pub mod literals;
#[cfg(feature = "mlir")]
mod mlir_entry;
pub mod parameters;
pub mod run_on;
pub mod signal;
pub mod stack;
pub mod types;
pub mod variables;
#[cfg(feature = "wasm")]
mod wasm_entry;

pub use crate::value::fiber::CallFrame;
pub use core::VM;

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::pipeline::CompileCtx;
use crate::value::{SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_SWITCH};
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        self.execute_proto(&Rc::new(bytecode.clone().into_proto()), None)
    }

    /// Mint a fresh `RuntimeRegion` from the activation's heap for a VM-produced
    /// *result* value: the result is the operation's own value (rc=1 after its
    /// single allocation), freed value-based by the consumer's `DecrefValueRegion`
    /// at the result's last use — the native-call result discipline. A *fresh*
    /// mint, never a region a tail-call is already freeing (a result born there
    /// would be freed under its reader — region-native-tail-return-uaf).
    ///
    /// Test-only: the VM error/result chokepoint ([`escaping_error`],
    /// [`set_error`], [`error_extra`], [`escaping_match_fail`]) builds through a
    /// `NativeCtx::new(self.heap())`, which mints+owns exactly such a fresh region
    /// and exposes the `ctx.*` allocation surface; this bare mint survives only for
    /// the pin tests that assert the contract.
    ///
    /// [`escaping_error`]: Self::escaping_error
    /// [`set_error`]: Self::set_error
    /// [`error_extra`]: Self::error_extra
    /// [`escaping_match_fail`]: Self::escaping_match_fail
    #[cfg(test)]
    pub(crate) fn result_region(&mut self) -> crate::hir::region::RuntimeRegion {
        self.heap().new_runtime_region()
    }

    /// Build an escaping error value born in a fresh region of its own, for
    /// VM-dispatch sites (arity check, runtime `eval`) that produce an error
    /// *result* outside any native-call `NativeCtx`.
    pub(crate) fn escaping_error(&mut self, kind: &str, msg: impl Into<String>) -> Value {
        let ctx = crate::primitives::ctx::Alloc::new(self.heap());
        ctx.error(kind, msg)
    }

    /// The VM-scope rich-error routine (docs/impl/region/errors.md): build
    /// `{:error :kind :message msg …extra}` in a fresh result region,
    /// freed value-based by the consumer's `DecrefValueRegion`. Same name as
    /// [`NativeCtx::error_extra`](crate::primitives::ctx::Alloc::error_extra)
    /// so `rich_error!` is uniform over `ctx` and `self`. The `extra` field
    /// values must be born in the same region — immediates (keywords/ints) or
    /// pass-throughs (incref'd by `alloc`'s content scan); a VM site has no
    /// `string` of its own to misplace.
    pub(crate) fn error_extra(
        &mut self,
        kind: &str,
        msg: impl Into<String>,
        extra: &[(&str, Value)],
    ) -> Value {
        let ctx = crate::primitives::ctx::Alloc::new(self.heap());
        ctx.error_extra(kind, msg, extra)
    }

    /// The runtime no-match error for `match`, born in a fresh region of its own.
    pub(crate) fn escaping_match_fail(&mut self, val: Value) -> Value {
        let ctx = crate::primitives::ctx::Alloc::new(self.heap());
        ctx.match_fail(val)
    }

    /// Set an error signal on the current fiber, the error value built through a
    /// `NativeCtx` over the VM's heap (docs/impl/region/ctx.md), which mints and
    /// owns its own fresh result region. The error escapes as the fiber's signal
    /// payload and is freed value-based by the consumer's `DecrefValueRegion`.
    pub(crate) fn set_error(&mut self, kind: &str, msg: impl Into<String>) {
        let ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *self.heap_ptr });
        let err = ctx.error(kind, msg);
        self.fiber.signal = Some((SIG_ERROR, err));
    }

    /// Check arity and set error signal if mismatch.
    /// Returns true if arity is OK, false if there's a mismatch.
    pub(crate) fn check_arity(&mut self, arity: &crate::value::Arity, arg_count: usize) -> bool {
        let mismatch = match arity {
            crate::value::Arity::Exact(n) if arg_count != *n => {
                Some(format!("expected {} arguments, got {}", n, arg_count))
            }
            crate::value::Arity::AtLeast(n) if arg_count < *n => Some(format!(
                "expected at least {} arguments, got {}",
                n, arg_count
            )),
            crate::value::Arity::Range(min, max) if arg_count < *min || arg_count > *max => Some(
                format!("expected {}-{} arguments, got {}", min, max, arg_count),
            ),
            _ => None,
        };

        if let Some(msg) = mismatch {
            let err = self.escaping_error("arity-error", msg);
            self.fiber.signal = Some((SIG_ERROR, err));
            return false;
        }
        true
    }

    /// Execute a code-object blueprint with an optional closure environment.
    ///
    /// Translation boundary: internally uses SignalBits, externally returns
    /// `Result<Value, String>`. A caller running a CLOSURE's body must use
    /// `Self::execute_code` (crate-private) with `closure.template.code()`
    /// instead: the executing-closure register's dispatch-entry invariant
    /// compares the register's code object to the executing `Code` by payload
    /// identity, and a fresh blueprint materializes a payload of its own.
    pub fn execute_proto(
        &mut self,
        proto: &Rc<crate::value::TemplateProto>,
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        // The blueprint carries the function's region tables with the rest of
        // its payload: the builder-idiom merge set the alloc dispatch
        // mint-or-reuses (docs/impl/region/merging.md § Merging), and the two
        // release tables an error exit walks (docs/impl/region/mechanism.md
        // § "An abandoned frame runs the releases it still owes").
        let code = crate::value::ClosureTemplate::for_proto(self.heap(), proto).code();
        self.execute_code(code, closure_env)
    }

    /// Execute a [`Code`](crate::value::Code) object at the root (with the
    /// tail-call and `SIG_SWITCH` trampolines), sharing the caller's `Rc`s. The
    /// entry for running a closure's body at the root — pass
    /// `closure.template.code()` (preserving the template's bytecode `Rc`, which
    /// the executing-closure register's dispatch-entry invariant compares by
    /// identity) and hand the closure through `pending_entry_closure`.
    pub(crate) fn execute_code(
        &mut self,
        code: crate::value::Code,
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        self.error_loc = None;

        let empty_env = Rc::new(vec![]);
        let mut current_code = code;
        let mut current_env = closure_env.cloned().unwrap_or(empty_env);

        // Install the executing-closure register for this body, bracketed
        // (save/restore) so a re-entrant driver — a native that loads a module
        // via `execute_proto` mid-activation — restores the outer
        // activation's register on return, exactly as
        // `execute_bytecode_saving_stack` brackets a closure body. The register
        // arrives through the one-shot `pending_entry_closure`: an entrant that
        // runs a CLOSURE's body through this raw entry (the spawned-worker body,
        // the stdlib exports call) sets it just before; a raw top-level/module
        // body sets nothing and runs untracked (NIL — no self-reference can
        // occur in non-closure bytecode).
        let saved_closure = self.fiber.current_closure;
        let entering = std::mem::replace(&mut self.pending_entry_closure, Value::NIL);
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(entering, &current_code);
        self.fiber.current_closure = entering;

        // Whether THIS invocation is the true root driver (the fiber's base
        // activation frame). A top-level body runs directly on the base slot —
        // this entry pushes no activation frame — so an `AdoptIntoActivation` it
        // executes mints the owner node in the BASE slot, and a top-level tail
        // call records its deferred release there, neither of which any
        // trampoline clean break ever reaches; the root driver discharges them
        // itself at the program's completion (below). A RE-ENTRANT execute_code
        // (a native loading a module mid-activation) runs in its caller's
        // activation (depth > 1), whose dues belong to that caller's own
        // completion release — they must not be touched here.
        let at_root = self.fiber.activation_dues.len() == 1;

        // Initial execution with tail-call loop.
        // Scope-mark rotation: when a tail call is rotation-safe,
        // release the previous iteration's temporaries via release().
        // The tail call's env (arguments) was built before release, so
        // referenced values survive. Only unreferenced temporaries are freed.
        let mut bits;
        let mut accumulated_squelch_mask = SignalBits::EMPTY;
        loop {
            let (b, _ip) = self.execute_bytecode_inner_impl(&current_code, &current_env, 0);
            bits = b;
            if let Some(tail) = self.pending_tail_call.take() {
                accumulated_squelch_mask |= tail.squelch_mask;
                // A top-level tail call re-enters the frame as the callee closure.
                #[cfg(debug_assertions)]
                Self::debug_assert_entry_closure_matches(tail.closure, &tail.code);
                self.fiber.current_closure = tail.closure;
                current_code = tail.code;
                current_env = tail.env;
            } else {
                if self.enforce_squelch(bits, accumulated_squelch_mask) {
                    bits = SIG_ERROR;
                }
                break;
            }
        }

        // Signal handling loop — handles SIG_SWITCH iteratively. Breaks with the
        // Result so the executing-closure register is restored once on the way out.
        let result: Result<Value, String> = loop {
            if bits.is_empty() {
                let (_, value) = self.fiber.signal.take().unwrap();
                break Ok(value);
            } else if bits == SIG_HALT {
                let (_, value) = self.fiber.signal.take().unwrap();
                // (halt) with no args → NIL → clean exit.
                // (halt <value>) or stack overflow → non-NIL → fatal error.
                if value == Value::NIL {
                    break Ok(value);
                }
                break Err(self.format_error_with_location(value));
            } else if bits.intersects(SIG_ERROR) {
                let (_, err_value) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                // Remember whether this uncaught error is a loud gate (:gated):
                // the top-level driver treats that as a skip, not a failure.
                // Always overwrite (Some or None) so a stale reason from an
                // earlier, since-caught gate never lingers.
                self.gated_exit_reason = gated_reason(err_value);
                break Err(self.format_error_with_location(err_value));
            } else if bits == SIG_SWITCH {
                bits = self.handle_sig_switch();
            } else {
                // Everything that is not an error, a halt, or the switch
                // trampoline arrives here with no handler left to run, and one
                // report answers for all of it: `:yield` is not privileged
                // among the bits that reach the root (docs/signals/protocol.md
                // § "Reaching the root"). The keywords are what the author of
                // the emitting call can act on; the mask alone is not. The
                // refused park is abandoned with its host.
                self.abandon_hosted_park(bits);
                self.fiber.signal.take();
                break Err(format!(
                    "Unhandled signal {} outside fiber context",
                    crate::signals::registry::format_bits(bits)
                ));
            }
        };
        // The root activation's clean break: discharge the base slot's dues —
        // the owner node (one tolerant decref → subtree drop over node +
        // adopted members) and whatever a top-level tail call deferred — at the
        // program's completion, the root counterpart of `trampoline_loop`'s
        // normal-break release (docs/impl/region/owner.md § "Owner nodes"). Runs
        // on every root exit — a finished program has no resumable state at this
        // boundary, so an error exit releases identically.
        if at_root {
            self.release_activation_dues();
        }
        self.fiber.current_closure = saved_closure;
        result
    }

    /// Handle a SIG_SWITCH signal: execute the pending fiber resume
    /// and resume the caller with the result. Returns the new signal bits.
    fn handle_sig_switch(&mut self) -> SignalBits {
        let pending = self
            .pending_fiber_resume
            .take()
            .expect("VM bug: SIG_SWITCH without pending_fiber_resume");
        let caller_frames = self.fiber.suspended.take().unwrap_or_default();
        self.fiber.signal.take();
        if self
            .runtime_config
            .has_trace_bit(crate::config::trace_bits::FIBER)
        {
            eprintln!(
                "[handle_sig_switch] caller_frames={} fiber_status={:?}",
                caller_frames.len(),
                pending.handle.with(|f| f.status),
            );
        }

        let (result_bits, result_value) =
            self.do_fiber_resume(&pending.handle, pending.fiber_value);

        let mask = pending.handle.with(|f| f.mask);

        self.finalize_if_halted(&pending.handle, result_bits);
        if result_bits.intersects(SIG_ERROR) {
            pending
                .handle
                .with_mut(|f| f.status = crate::value::FiberStatus::Error);
        }

        if self.absorbs(&pending.handle, mask, result_bits, result_value) {
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.resume_suspended(caller_frames, result_value)
        } else {
            self.fiber.signal = Some((result_bits, result_value));

            // Rebuild fiber.suspended for uncaught signals: the outer code
            // (execute_scheduled, execute_proto) needs the suspension chain
            // to resume after handling the signal (e.g., SIG_IO → sync I/O).
            // Prepend a FiberResume frame so resume_suspended can re-enter
            // the child fiber when the signal is handled.
            if !result_bits.intersects(SIG_ERROR) && !result_bits.intersects(SIG_HALT) {
                let fiber_resume_frame = SuspendedFrame::FiberResume {
                    handle: pending.handle.clone(),
                    fiber_value: pending.fiber_value,
                };
                let mut frames = vec![fiber_resume_frame];
                frames.extend(caller_frames);
                self.fiber.suspended = Some(frames);
            }

            result_bits
        }
    }

    /// Execute user bytecode under the async scheduler.
    ///
    /// Wraps the bytecode in a thunk and calls `(ev/run thunk)` to
    /// install the async scheduler. The thunk carries the bytecode's
    /// inferred signal so fiber scheduling and shared allocator
    /// provisioning work correctly.
    ///
    /// Falls back to direct execution if stdlib isn't loaded yet.
    pub fn execute_scheduled(
        &mut self,
        bytecode: &Bytecode,
        cctx: &CompileCtx,
    ) -> Result<Value, String> {
        let ev_run = match cctx.lookup_stdlib_value(crate::value::SymbolId::of("ev/run")) {
            Some(v) => v,
            None => return self.execute(bytecode),
        };

        // The entry thunk's blueprint is the program's own: it runs the top-level
        // bytecode, so it carries the real program's location table, nested-lambda
        // blueprints, and builder-idiom merge metadata (docs/impl/region/merging.md
        // § Merging). Without them the top-level merge would diverge from the
        // unit/embedding paths, which carry it. Empty unless a merge fired.
        let thunk_proto = Rc::new(bytecode.clone().into_proto());

        let call_region = crate::lir::lower::new_static_region();
        // The synthetic `Call` below is hand-encoded bytecode, so the slot is
        // written as its raw wire-format `u32` (the one legit `.get()` site —
        // a bytecode encoder).
        let call_region_slot = call_region.get();
        let synthetic_bc = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::LoadConst as u8,
            0,
            1,
            Instruction::Call as u8,
            0,
            1, // arg_count = 1 (u16be)
            (call_region_slot >> 24) as u8,
            (call_region_slot >> 16) as u8,
            (call_region_slot >> 8) as u8,
            (call_region_slot & 0xff) as u8, // region_id (u32be)
            Instruction::Return as u8,
        ];

        // `call_region` is the static slot baked into the synthetic Call above;
        // it doubles as the physical region the entry thunk is born in (the
        // static/runtime conflation flagged elsewhere — preserved here). The slot
        // counter starts at 2, so nonzero.
        let entry_region = crate::hir::region::RuntimeRegion::new(call_region_slot)
            .expect("call_region slot nonzero");
        // Build the entry thunk as an ordinary allocation into `entry_region`
        // (mortal) — reclaimed by the termination sweep. The synthetic
        // `(ev/run thunk)` bytecode has no MakeClosure of its own; the real
        // program's nested lambdas ride on the thunk blueprint's child_protos and
        // resolve when `ev/run` calls the thunk. The thunk names its region
        // explicitly and the wrapper's allocating opcodes resolve their own
        // static region slots.
        let thunk = {
            let heap = self.heap();
            let template = crate::value::closure::materialize(heap, &thunk_proto, entry_region);
            crate::value::build::closure(
                heap,
                crate::value::Closure::new(
                    crate::value::TemplateRef::region(template),
                    crate::value::region_slice::RegionSlice::empty(),
                    SignalBits::EMPTY,
                ),
                entry_region,
            )
        };
        let synthetic_constants = vec![thunk, ev_run];
        // The synthetic `(ev/run thunk)` wrapper has no allocations and no releases
        // of its own; the real program's tables ride the thunk blueprint and
        // resolve when `ev/run` calls the thunk.
        let wrapper = crate::value::TemplateProto::new(
            synthetic_bc,
            crate::value::Arity::Exact(0),
            synthetic_constants,
        );
        self.execute_proto(&Rc::new(wrapper), None)
    }
}

/// If `err_value` is a loud-gate signal `{:error :gated :reason …}`, return its
/// reason (empty string when the `:reason` field is absent). Any other value —
/// including ordinary errors — returns `None`, so only intentional gates are
/// ever treated as skips. See `VM::gated_exit_reason`.
/// The reason string of a `(gate! …)` skip signal, or `None` for any other
/// value. A `:gated` error is an intentional SKIP (an unbuilt plugin/feature),
/// not a failure — both the VM driver and the WASM tier's `run_module` treat it
/// as a clean exit rather than a runtime error.
pub(crate) fn gated_reason(err_value: Value) -> Option<String> {
    let entries = err_value.as_struct()?;
    let mut is_gated = false;
    let mut reason = String::new();
    for (key, value) in entries {
        let crate::value::types::TableKey::Keyword(hash) = key else {
            continue;
        };
        match *hash {
            h if h == crate::value::keyword::keyword_hash("error")
                && value.is_keyword_named("gated") =>
            {
                is_gated = true;
            }
            h if h == crate::value::keyword::keyword_hash("reason") => {
                if let Some(s) = value.with_string(|s| s.to_string()) {
                    reason = s;
                }
            }
            _ => {}
        }
    }
    if is_gated {
        Some(reason)
    } else {
        None
    }
}

// ── The VM result-region seam ───────────────────────────────────────
//
// `result_region()` mints a fresh, reclaimable region from the activation's heap
// for a VM-internal result value; the result is freed value-based by the
// consumer's `DecrefValueRegion` (docs/impl/region/ctx.md). These pins fix its
// contract.

#[cfg(test)]
mod result_region_tests;
