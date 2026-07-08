//! Bytecode execution entry points.
//!
//! ## Re-entrancy
//!
//! `execute_bytecode_saving_stack` makes the VM re-entrant. It is called
//! recursively from within the dispatch loop in several places:
//!
//! | Caller | Context |
//! |--------|---------|
//! | `eval` primitive | Compiles and runs Elle source from within running code |
//! | Non-yielding `fiber/resume` | Runs a child fiber inline on the current thread |
//! | `arena/allocs` SIG_QUERY handler | Runs a thunk to measure its allocations |
//! | JIT trampolines | Re-enters interpreter for uncompiled hot paths |
//! | Fiber resume in `call.rs` | Resumes a suspended fiber |
//!
//! ### What `execute_bytecode_saving_stack` preserves
//!
//! - **Operand stack**: saved before inner execution, restored after. The
//!   inner execution sees an empty stack. The outer stack is invisible to it.
//! - **Active allocator pointer**: saved and restored. Inner execution uses
//!   whatever allocator was active (scope bumps, shared allocator, etc.).
//!
//! ### What it does NOT preserve
//!
//! - **`self.fiber.signal`**: the inner execution overwrites this with its
//!   result. Callers must read `fiber.signal` immediately after return and
//!   before any other operation that might set it.
//! - **`self.fiber.frames` / `self.fiber.call_stack`**: inner calls push
//!   and pop frames. On normal return these are balanced. On error they
//!   may be partially unwound.
//! - **`self.error_loc`**: overwritten by inner execution on error.
//! - **`self.pending_tail_call`**: consumed by the tail-call loop inside
//!   `execute_bytecode_saving_stack`. Never leaks to the outer caller.
//!
//! ### Yield from inner execution
//!
//! If the inner closure yields (`SIG_YIELD`), `execute_bytecode_saving_stack`
//! returns `SIG_YIELD` to its caller. The saved outer stack is restored, but
//! the fiber is now suspended mid-inner-execution. **This is a bug in any
//! caller that does not handle `SIG_YIELD`.** Current callers that call
//! user-provided closures (`eval`, `arena/allocs`) do not handle yield —
//! they propagate the signal upward, which will confuse the outer execution
//! context. Closures passed to these primitives must be non-yielding (Pure
//! signal). This is not currently enforced at the call site.
//!
//! ### Nested `fiber/resume` — the SIG_SWITCH obligation
//!
//! A thunk that calls `fiber/resume` is NOT "yielding" in the sense above —
//! it does not suspend its own caller — yet it still needs special handling.
//! User code always runs inside a fiber (the async scheduler resumes the
//! program in one), so `current_fiber_handle` is `Some` throughout. A
//! `fiber/resume` reached with an enclosing fiber does NOT run the child
//! inline: to avoid growing the Rust stack per nesting level,
//! `handle_fiber_resume_signal` suspends the *caller's* continuation and
//! returns `SIG_SWITCH`, handing the child to a driving trampoline
//! (`handle_sig_switch`). The top-level dispatch loop ([`VM::execute_bytecode`])
//! is that trampoline at the root; a re-entrant boundary that runs a thunk on
//! the current fiber must be one too. If it is not, the `SIG_SWITCH` unwinds
//! straight out of `execute_bytecode_saving_stack` and the thunk's continuation
//! is later resumed by the *outer* trampoline — i.e. OUTSIDE the re-entrant
//! caller's scope. For `arena/allocs` that meant the measurement returned the
//! resumed child's value instead of `(result . net)` and never finished the
//! thunk (`tests/elle/arena.lisp` "arena/allocs measures a thunk that resumes
//! a fiber"; the `fiber-spawn-10` scenario in `tests/elle/resource.lisp`).
//!
//! `VM::run_thunk_to_completion` is the safe entry point: it drives
//! `SIG_SWITCH` to completion exactly as the root loop does, so a nested
//! `fiber/resume` runs fully and the thunk produces its real result. Prefer it
//! over a raw `execute_bytecode_saving_stack` for any caller that runs a thunk
//! as part of the *current* fiber's execution (`eval`, `arena/allocs`, the
//! test-setup module loader). Do NOT use it when running a *child fiber's* body
//! (`do_fiber_first_resume`): there `SIG_SWITCH` must propagate to the child's
//! own driving `do_fiber_resume`, not be driven here.
//!
//! ### Rules for new callers
//!
//! If you add a new SIG_QUERY handler or primitive that calls a user closure
//! via `execute_bytecode_saving_stack`:
//!
//! 1. Read `fiber.signal` immediately after return to get the result.
//! 2. Check `exec_result.bits` for `SIG_ERROR` and `SIG_HALT` before using
//!    the result.
//! 3. Do NOT call it with a closure that may yield unless you handle
//!    `SIG_YIELD` in the return value.
//! 4. Do NOT assume `fiber.signal` is unchanged after the call.
//! 5. The inner execution runs on the SAME fiber — same heap, same
//!    parameter frames. It is not isolated.
//! 6. If the closure may call `fiber/resume`, run it through
//!    `VM::run_thunk_to_completion` (not raw `execute_bytecode_saving_stack`)
//!    so the `SIG_SWITCH` trampoline is driven inside your scope — see the
//!    SIG_SWITCH section above.

use crate::value::{SignalBits, Value, SIG_ERROR};
use std::rc::Rc;

use super::core::VM;

/// Result of `execute_bytecode_saving_stack`.
///
/// Contains the signal, IP, the active bytecode/constants/env at exit, and
/// the inner operand stack at the moment of suspension.
///
/// When a tail call occurs before a signal, the active context differs from
/// the original closure — callers that create `SuspendedFrame`s must use
/// these fields, not the original closure's bytecode/constants.
///
/// `stack` captures the inner execution's operand stack at suspension time.
/// This is essential for fuel-pause resumption: when `SIG_FUEL` fires at a
/// `TailCall` or `Call` instruction, the args are still on the stack. On
/// resume the instruction re-executes from `ip`, so the stack must be
/// restored exactly as it was.  `SIG_YIELD` is exempt — `handle_yield`
/// drains the stack into `fiber.suspended` before returning, so
/// `fiber.suspended` is already populated and the `stack` field here is
/// unused for that signal.
pub(crate) struct ExecResult {
    pub bits: SignalBits,
    pub ip: usize,
    /// The active code object at exit (may differ from the input if a tail call
    /// occurred before the signal). The template-derived half of the context.
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The inner operand stack at suspension. Populated by
    /// `execute_bytecode_saving_stack`; empty for `execute_bytecode_from_ip`.
    pub stack: Vec<Value>,
    /// This activation's static→physical region remap, captured by
    /// `execute_bytecode_saving_stack` just before it pops the frame on a
    /// suspending exit. Callers that build a `SuspendedFrame::Bytecode` from
    /// the callee's returned context (the inner/fuel-pause frame) attach this
    /// so the remap survives the yield. Default (empty) for
    /// `execute_bytecode_from_ip`, whose caller manages frames itself.
    pub activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::RuntimeRegion>,
    /// This activation's owner node, TAKEN by `execute_bytecode_saving_stack`
    /// beside `activation_region_map` on a non-OK exit — the channel that
    /// carries the node out of the already-popped activation to the caller
    /// that builds its park (`BytecodeFrame::activation_owner_node`): the
    /// fiber body's pause in `do_fiber_first_resume`, the interrupted callee's
    /// inner frame in `call_inner`. A suspend handler that parked the frame
    /// itself (the yield path) already took the node, so this reads `None`
    /// there — the move discipline holds. Always `None` for
    /// `execute_bytecode_from_ip` (`resume_suspended` manages the slot
    /// directly).
    pub activation_owner_node: Option<crate::hir::region::RuntimeRegion>,
    /// The executing-closure register (`fiber.current_closure`) at the moment the
    /// trampoline broke — the callee's value, possibly re-installed by tail calls
    /// in this activation. A caller building a `SuspendedFrame` from this returned
    /// context parks it so the self-identity survives the yield. `NIL` for an
    /// untracked activation.
    pub current_closure: Value,
}

impl VM {
    /// Debug-only: the executing-closure register handed to a body entry must
    /// name that body — its template bytecode must be the very `Rc` the entered
    /// `Code` carries. Called ONLY where the closure is live by construction
    /// (the entrant just took `code` from it), never on a restored/parked
    /// register — a parked register is a possibly-dead borrow (the region
    /// solver frees a closure value at its last use while its activation's
    /// `code`/`env` live on as `Rc`s), so dereferencing it is unsound.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_entry_closure_matches(entering: Value, code: &crate::value::Code) {
        if let Some(cl) = entering.as_closure() {
            debug_assert!(
                std::rc::Rc::ptr_eq(&cl.template.bytecode, &code.bytecode),
                "executing-closure register mismatch at body entry: the entrant handed \
                 a closure whose body (bytecode {:p}) is not the body being entered \
                 (bytecode {:p})",
                std::rc::Rc::as_ptr(&cl.template.bytecode),
                std::rc::Rc::as_ptr(&code.bytecode),
            );
        }
    }

    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    ///
    /// Returns `ExecResult` containing the signal, IP, and the active
    /// bytecode/constants/env at exit. The active context may differ from
    /// the input if a tail call occurred before the signal.
    /// Core tail-call trampoline loop shared by `execute_bytecode_from_ip`
    /// and `execute_bytecode_saving_stack`.
    fn trampoline_loop(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> ExecResult {
        let mut current_code = code.clone();
        let mut current_env = closure_env.clone();
        let mut current_ip = start_ip;
        let mut accumulated_squelch_mask = SignalBits::EMPTY;
        // Closure-callee regions adopted from frame-replacing tail calls in this
        // activation (`TailCallInfo::adopt_region`). A tail call's callee closure
        // is a per-call local allocation whose compiler-emitted release is dead
        // past the `TailCall`; this activation took over that release and runs it
        // on NORMAL completion below. Deduped by region: a tail-recursive `go`
        // re-enters with the SAME closure each iteration but carries one dead
        // decref, so it must be released exactly once. Released only on the clean
        // break — on a suspending/error/squelch exit the closures may still be
        // needed (resume) and are left to leak, a bounded over-keep, never a
        // double-free (unlike the activation owner node, these regions are
        // `Counted` and not moved into the park, so no discard-time release can
        // be sound without per-frame liveness).
        let mut adopted_closures: Vec<crate::hir::region::RuntimeRegion> = Vec::new();

        loop {
            let (bits, ip) =
                self.execute_bytecode_inner_impl(&current_code, &current_env, current_ip);

            if !bits.is_ok() {
                if self.enforce_squelch(bits, accumulated_squelch_mask) {
                    break ExecResult {
                        bits: SIG_ERROR,
                        ip,
                        code: current_code,
                        env: current_env,
                        stack: vec![],
                        activation_region_map: rustc_hash::FxHashMap::default(),
                        activation_owner_node: None,
                        current_closure: self.fiber.current_closure,
                    };
                }
                let inner_stack = std::mem::take(&mut self.fiber.stack).into_vec();
                break ExecResult {
                    bits,
                    ip,
                    code: current_code,
                    env: current_env,
                    stack: inner_stack,
                    activation_region_map: rustc_hash::FxHashMap::default(),
                    activation_owner_node: None,
                    current_closure: self.fiber.current_closure,
                };
            }

            if let Some(tail) = self.pending_tail_call.take() {
                accumulated_squelch_mask |= tail.squelch_mask;
                if let Some(r) = tail.adopt_region {
                    if !adopted_closures.contains(&r) {
                        adopted_closures.push(r);
                    }
                }
                // The frame is reused in place but now runs the tail callee: track
                // it as the executing closure so a self-edge resolved after this
                // replacement names the right closure (a self-recursive `loop`
                // re-installs itself; a tail call to a sibling installs the sibling).
                #[cfg(debug_assertions)]
                Self::debug_assert_entry_closure_matches(tail.closure, &tail.code);
                self.fiber.current_closure = tail.closure;
                current_code = tail.code;
                current_env = tail.env;
                current_ip = 0;
            } else {
                // Normal completion: run the deferred closure-callee releases —
                // the decrefs the frame-replacing tail calls left dead.
                for &r in &adopted_closures {
                    self.heap().decref_region_if_present(r);
                }
                // ... and free this activation's owner node, if an
                // `AdoptIntoActivation` minted one: the node's single decref
                // subtree-drops every member the activation adopted
                // (docs/impl/region/owner.md § "Owner nodes"). The same
                // clean-break discipline as the adopted-closure releases above —
                // a frame-replacing tail call keeps the activation (and its
                // node) alive to the recursion's completion here.
                self.release_activation_owner_node();
                break ExecResult {
                    bits,
                    ip,
                    code: current_code,
                    env: current_env,
                    stack: vec![],
                    activation_region_map: rustc_hash::FxHashMap::default(),
                    activation_owner_node: None,
                    current_closure: self.fiber.current_closure,
                };
            }
        }
    }

    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming fibers from where they suspended.
    pub(crate) fn execute_bytecode_from_ip(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> ExecResult {
        self.trampoline_loop(code, closure_env, start_ip)
    }

    /// Execute bytecode returning SignalBits (for fiber/closure execution).
    /// The result value is stored in `self.fiber.signal`.
    ///
    /// Saves/restores the caller's stack around execution.
    /// Handles pending tail calls in a loop.
    pub(crate) fn execute_bytecode_saving_stack(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
    ) -> ExecResult {
        let saved_stack = std::mem::take(&mut self.fiber.stack);
        // Install the executing-closure register for this activation, mirroring the
        // region-map push/pop below. The caller (the interpreter call path) sets the
        // one-shot `pending_entry_closure` immediately before this call; take it
        // (resetting to NIL) and save the caller's register to restore on return. A
        // caller that set nothing enters NIL — the body runs untracked. The
        // trampoline re-installs it on each tail-call frame replacement; on a
        // suspending exit `result.current_closure` carries the value at suspend for
        // the caller to park.
        let saved_closure = self.fiber.current_closure;
        let entering = std::mem::replace(&mut self.pending_entry_closure, Value::NIL);
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(entering, code);
        self.fiber.current_closure = entering;
        // Each closure-body execution is a fresh activation: push a
        // region-remap frame so the body's static region slots map to fresh
        // physical regions (docs/regions/semantics.md — every value its own region).
        // TCO loops inside `trampoline_loop` without re-entering here, so a
        // tail call correctly reuses the frame.
        // On a suspending exit (yield/IO/signal) this frame is popped as the
        // Rust call stack unwinds. Capture it into the result first so a
        // suspending caller can attach it to the `SuspendedFrame::Bytecode` it
        // builds from this activation's returned context (cross-yield remap
        // preservation — docs/impl/region/model.md). TCO loops inside `trampoline_loop`
        // without re-entering here, so a tail call correctly reuses the frame.
        self.push_activation_region_map();
        let mut result = self.trampoline_loop(code, closure_env, 0);
        if !result.bits.is_ok() {
            result.activation_region_map = self
                .fiber
                .activation_region_maps
                .last()
                .cloned()
                .unwrap_or_default();
            // MOVE the activation's owner node out with the map: a suspend
            // handler that parked a frame already took it (this reads None),
            // but a pause with no frame of its own (fuel) leaves it here, and
            // the caller that builds the park from this result re-attaches it
            // (docs/impl/region/owner.md § "Owner nodes").
            result.activation_owner_node = self.take_activation_owner_node();
        }
        self.pop_activation_region_map();
        // Restore the caller's executing-closure register. On a suspending exit the
        // callee's value is already in `result.current_closure` (stamped by the
        // trampoline); on normal return the caller resumes as itself.
        self.fiber.current_closure = saved_closure;
        self.fiber.stack = saved_stack;
        result
    }

    /// Run a thunk on the CURRENT fiber to completion, driving the
    /// fiber-resume (`SIG_SWITCH`) trampoline — the safe entry for re-entrant
    /// callers whose thunk is part of *this* fiber's execution (`eval`,
    /// `arena/allocs`, the test-setup module loader).
    ///
    /// It wraps [`Self::execute_bytecode_saving_stack`] with the same
    /// `SIG_SWITCH`-draining loop the root dispatch ([`VM::execute_bytecode`])
    /// runs. A `fiber/resume` inside the thunk, with the program itself inside a
    /// fiber (the async scheduler always runs user code in one), suspends the
    /// thunk's continuation and returns `SIG_SWITCH` for a driving trampoline
    /// rather than executing inline (`handle_fiber_resume_signal`). Without
    /// driving it here the switch unwinds out of the re-entrant boundary and the
    /// continuation resumes OUTSIDE the caller's scope — the `arena/allocs`
    /// measurement returned the resumed child's value instead of `(result .
    /// net)` and never finished the thunk (`tests/elle/arena.lisp`,
    /// `tests/elle/resource.lisp` `fiber-spawn-10`).
    ///
    /// Returns the final signal bits; the result value is left in
    /// `self.fiber.signal` (read it immediately — see the re-entrancy rules in
    /// the module docs). A genuinely suspending thunk (yield / I/O) returns its
    /// suspending bits unchanged; callers that forbid suspension act on that.
    ///
    /// NOT for running a *child fiber's* body (`do_fiber_first_resume`): there
    /// `SIG_SWITCH` must propagate to the child's own driving `do_fiber_resume`,
    /// not be drained here.
    pub(crate) fn run_thunk_to_completion(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
    ) -> SignalBits {
        let mut bits = self.execute_bytecode_saving_stack(code, closure_env).bits;
        while bits == crate::value::SIG_SWITCH {
            bits = self.handle_sig_switch();
        }
        bits
    }
}
