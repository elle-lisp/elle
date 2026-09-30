// audited: 2026-09-29
//! The VM's execution entries: a blueprint, a code object at the root, and a
//! program under the async scheduler. The module list sits above them.
//!
//! docs/impl/vm.md
//! docs/impl/region/rules.md

pub mod arithmetic;
pub mod call;
pub mod capture;
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
// Not pub: jit_entry only adds `impl VM` methods.
#[cfg(feature = "jit")]
mod jit_entry;
pub mod literals;
#[cfg(feature = "mlir")]
mod mlir_entry;
pub(crate) mod native_stack;
pub mod parameters;
pub mod run_on;
mod scheduled;
pub mod signal;
pub mod stack;
pub mod types;
pub mod variables;
#[cfg(feature = "wasm")]
mod wasm_entry;

pub use crate::value::fiber::CallFrame;
pub use core::VM;

use crate::compiler::bytecode::Bytecode;
use crate::value::fiber::TailSquelch;
use crate::value::{Value, SIG_ERROR, SIG_HALT, SIG_SWITCH};
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
    /// would be freed under its reader).
    ///
    /// Test-only: the VM error/result chokepoint ([`escaping_error`],
    /// [`set_error`], [`error_extra`], [`escaping_match_fail`]) builds through an
    /// `Alloc::new(self.heap())`, which mints+owns exactly such a fresh region
    /// and exposes the `ctx.*` allocation surface; this bare mint exists only for
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
    /// [`Alloc::error_extra`](crate::primitives::ctx::Alloc::error_extra)
    /// so `rich_error!` is uniform over `ctx` and `self`. The `extra` field
    /// values must be born in the same region — immediates (keywords/ints) or
    /// pass-throughs (incref'd by `alloc`'s content scan); a VM site has no
    /// `string` of its own to misplace. Its one caller is a tier's refusal, so a
    /// build with no optional tier leaves it unused.
    #[cfg_attr(
        not(any(feature = "jit", feature = "wasm", feature = "mlir")),
        allow(dead_code)
    )]
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

    /// Set an error signal on the current fiber, the error value built through an
    /// `Alloc` over the VM's heap (docs/impl/region/ctx.md), which mints and
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
        if arity.matches(arg_count) {
            return true;
        }

        let msg = match arity {
            crate::value::Arity::Exact(n) => {
                format!("expected {} arguments, got {}", n, arg_count)
            }
            crate::value::Arity::AtLeast(n) => {
                format!("expected at least {} arguments, got {}", n, arg_count)
            }
            crate::value::Arity::Range(min, max) => {
                format!("expected {}-{} arguments, got {}", min, max, arg_count)
            }
        };
        let err = self.escaping_error("arity-error", msg);
        self.fiber.signal = Some((SIG_ERROR, err));
        false
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
        // mint-or-reuses (docs/impl/region/merging.md), and the two release
        // tables an error exit walks (docs/impl/region/mechanism.md).
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
        // The body addresses its locals from stack position 0, so it runs on a
        // stack of its own and the one found here goes back at the end
        // (docs/impl/vm.md § "Every body starts on an empty operand stack").
        let saved_stack = std::mem::take(&mut self.fiber.stack);

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
        // Where a refused park truncates the `parameterize` frames to.
        let depth = self.fiber.param_depth();

        // Initial execution with tail-call loop: a top-level tail call replaces
        // the body in place.
        let mut bits;
        let mut tail_squelch = TailSquelch::none(depth);
        loop {
            bits = self.run_dispatch(&current_code, &current_env, 0).bits;
            if let Some(tail) = self.pending_tail_call.take() {
                tail_squelch.add(tail.squelch_mask, self.fiber.param_depth());
                // A top-level tail call re-enters the frame as the callee closure.
                #[cfg(debug_assertions)]
                Self::debug_assert_entry_closure_matches(tail.closure, &tail.code);
                self.fiber.current_closure = tail.closure;
                current_code = tail.code;
                current_env = tail.env;
            } else {
                if self.enforce_squelch(bits, tail_squelch.mask, tail_squelch.depth) {
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
                // among the bits that reach the root (docs/signals/protocol.md).
                // The keywords are what the author of
                // the emitting call can act on; the mask alone is not. The
                // root driver cannot hold the park, so it refuses it.
                self.refuse_hosted_park(bits, depth);
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
        // normal-break release (docs/impl/region/owner.md). Runs
        // on every root exit — a finished program has no resumable state at this
        // boundary, so an error exit releases identically.
        if at_root {
            self.release_activation_dues();
        }
        self.root_exit_depth = self.fiber.stack.len();
        self.fiber.stack = saved_stack;
        self.fiber.current_closure = saved_closure;
        result
    }
}

/// If `err_value` is a loud-gate signal `{:error :gated :reason …}`, return its
/// reason (empty string when the `:reason` field is absent). Any other value —
/// including ordinary errors — returns `None`, so only intentional gates are
/// ever treated as skips. A `:gated` error is an intentional SKIP (an unbuilt
/// plugin or feature), not a failure: both the VM driver and the WASM tier's
/// `run_module` treat it as a clean exit. See `VM::gated_exit_reason`.
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
