// audited: 2026-09-28
//! Bytecode execution entry points, the tail-call trampoline, and the opening
//! and closing of an activation.
//!
//! docs/impl/vm.md
//! docs/impl/region/relocate.md
//!
//! An interpreted non-tail call does not come back through here: the callee
//! runs on its caller's dispatch loop, in `nested.rs`. What does come through
//! `execute_bytecode_saving_stack` is **re-entry** — a primitive or a compiled
//! function running a closure from Rust. The "Re-entrancy" section of
//! src/vm/AGENTS.md lists the re-entrant callers.
//!
//! ### What `execute_bytecode_saving_stack` preserves
//!
//! - **Operand stack**: saved before inner execution, restored after. The
//!   inner execution sees an empty stack. The outer stack is invisible to it.
//! - **Executing-closure register**: saved, and restored on the way out.
//! - **Parameter frames**, on an error that abandons the body: the frames the
//!   body pushed and never popped are dropped (`drop_abandoned_param_frames`).
//!
//! ### What it does NOT preserve
//!
//! - **`self.fiber.signal`**: the inner execution overwrites this with its
//!   result. Callers must read `fiber.signal` immediately after return and
//!   before any other operation that might set it.
//! - **`self.fiber.call_stack`**: inner calls push and pop trace frames. On
//!   normal return these are balanced. On error they may be partially unwound.
//! - **`self.error_loc`**: overwritten by inner execution on error.
//! - **`self.pending_tail_call`**: consumed by the tail-call loop inside
//!   `execute_bytecode_saving_stack`. Never leaks to the outer caller.
//!
//! Each re-entry nests on the Rust stack, so it halts with `:stack-overflow`
//! rather than enter while less than `native_stack::REENTRY_RESERVE` is left.
//!
//! ### A suspend from inner execution
//!
//! If the inner closure suspends (a yield, an I/O request),
//! `execute_bytecode_saving_stack` returns the suspending bits with the outer
//! stack restored. No host resumes the inner continuation, so each refuses
//! the suspend: `eval`, `import` and the test-setup loader report an error,
//! and `arena/allocs` returns the signal from its own call. Each first calls
//! `VM::abandon_hosted_park`, because the refused park is dead.
//!
//! ### Nested `fiber/resume` — the SIG_SWITCH obligation
//!
//! A thunk that calls `fiber/resume` does not suspend its own caller, yet it
//! still needs special handling.
//! User code always runs inside a fiber (the async scheduler resumes the
//! program in one), so `current_fiber_handle` is `Some` throughout. A
//! `fiber/resume` reached with an enclosing fiber does NOT run the child
//! inline: to avoid growing the Rust stack per nesting level,
//! `handle_fiber_resume_signal` suspends the *caller's* continuation and
//! returns `SIG_SWITCH`, handing the child to a driving trampoline
//! (`handle_sig_switch`). The top-level dispatch loop ([`VM::execute_proto`])
//! is that trampoline at the root; a re-entrant boundary that runs a thunk on
//! the current fiber must be one too. If it is not, the `SIG_SWITCH` unwinds
//! straight out of `execute_bytecode_saving_stack` and the thunk's continuation
//! is later resumed by the *outer* trampoline — that is, OUTSIDE the
//! re-entrant caller's scope. An `arena/allocs` measurement would then answer
//! with the resumed child's value instead of `(result . net)` and never finish
//! the thunk (`tests/elle/arena.lisp` "arena/allocs measures a thunk that resumes
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
//! 3. If the closure may suspend, refuse the suspending bits and call
//!    `VM::abandon_hosted_park`, as the section above describes.
//! 4. Do NOT assume `fiber.signal` is unchanged after the call.
//! 5. The inner execution runs on the SAME fiber — same heap, same
//!    parameter frames. It is not isolated.
//! 6. If the closure may call `fiber/resume`, run it through
//!    `VM::run_thunk_to_completion` (not raw `execute_bytecode_saving_stack`)
//!    so the `SIG_SWITCH` trampoline is driven inside your scope — see the
//!    SIG_SWITCH section above.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};
use std::rc::Rc;

use super::core::VM;

mod exit;
mod nested;

pub(crate) use exit::{ExecResult, Exit};

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
            // Identity is the payload's backing address: every header from one
            // blueprint shares it (docs/impl/region/template.md), so two code
            // objects for the same function compare equal however each was
            // built, and two different functions never do.
            debug_assert!(
                std::ptr::eq(cl.template.bytecode().as_ptr(), code.bytecode().as_ptr()),
                "executing-closure register mismatch at body entry: the entrant handed \
                 a closure whose body (bytecode {:p}) is not the body being entered \
                 (bytecode {:p})",
                cl.template.bytecode().as_ptr(),
                code.bytecode().as_ptr(),
            );
        }
    }

    /// Replace the running activation's body with a pending tail call's callee,
    /// answering the callee's code and environment. The caller ORs the tail
    /// call's squelch mask into the activation's own.
    fn replace_by_tail_call(
        &mut self,
        tail: crate::vm::core::TailCallInfo,
    ) -> (crate::value::Code, Rc<Vec<Value>>) {
        // The fresh-frame invariant (docs/impl/region/rules.md Rule 5): the
        // callee's unwritten local slots must read NIL exactly as on a fresh
        // activation — a branch-arm temp's scope-end release reads its slot
        // unconditionally and no-ops only on NIL. The reused stack still holds
        // the caller's locals at those indices (all dead: released at last use or
        // moved into the callee), so drop them to the frame base before the
        // callee runs (runtime::tests::ownership::frame).
        self.fiber.stack.truncate(self.current_frame_base());
        // The frame is reused in place but now runs the tail callee: track it as
        // the executing closure so a self-edge resolved after this replacement
        // names the right closure (a self-recursive `loop` re-installs itself; a
        // tail call to a sibling installs the sibling).
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(tail.closure, &tail.code);
        self.fiber.current_closure = tail.closure;
        (tail.code, tail.env)
    }

    /// Close out an activation whose dispatch loop exited at `exit`, answering
    /// its `ExecResult`.
    ///
    /// On a signal: a squelch the activation's tail calls accumulated turns the
    /// signal into an error, an error runs the abandoned-frame walk when
    /// `walk_abandoned` says the frame is abandoned, and the operand stack
    /// leaves in the result. On a clean return: the activation discharges what
    /// it owes.
    ///
    /// `walk_abandoned` — run the releases this activation still owes when it
    /// leaves by an **error** (docs/impl/region/mechanism.md § "An abandoned
    /// frame runs the releases it still owes"). False where the frame is not
    /// abandoned: a fiber body whose entrant parks it for the restarts system,
    /// and the resume entry, whose frame the caller manages and may re-park.
    fn end_activation(
        &mut self,
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        exit: Exit,
        tail_squelch: SignalBits,
        walk_abandoned: bool,
    ) -> ExecResult {
        if exit.bits.is_empty() {
            // Normal completion: discharge what this activation owes — the
            // decrefs its frame-replacing tail calls left dead, and its owner
            // node, whose single decref subtree-drops every member the
            // activation adopted (docs/impl/region/owner.md § "Owner nodes").
            // One clean-break discipline for both: a frame-replacing tail call
            // keeps the activation alive to the recursion's completion here,
            // and so keeps everything it owes.
            self.release_activation_dues();
            return ExecResult::ended(exit, code, env, vec![], self.fiber.current_closure);
        }
        // A squelch/attune boundary turns the signal into an error this
        // activation never catches, so this exit IS the error exit and is
        // written as one — a second arm would be a second place to keep the
        // abandonment accounting in step (docs/impl/region/mechanism.md § "A
        // squelch boundary abandons frames the same way, so it runs the same
        // walk").
        let exit = if self.enforce_squelch(exit.bits, tail_squelch) {
            Exit::at(SIG_ERROR, exit.ip)
        } else {
            exit
        };
        let bits = exit.bits;
        // The frame's locals are still on the stack, and an error leaves
        // through the signal machinery without running the rest of its
        // instructions — so the releases among them run here, before the
        // locals travel out in the result. The releases this activation took
        // over from a frame-replacing tail call are owed on the same question
        // and have no table to be read off, their emitting instruction having
        // died with the replaced frame (docs/impl/region/owner.md § "What an
        // abandoned frame owes, it owes the deferred set too").
        if walk_abandoned && bits.intersects(SIG_ERROR) {
            let payload = self.fiber.signal.map(|(_, v)| v).unwrap_or(Value::NIL);
            self.release_abandoned_frame(&code, payload);
            self.release_abandoned_deferred();
        }
        let stack = std::mem::take(&mut self.fiber.stack).into_vec();
        ExecResult::ended(exit, code, env, stack, self.fiber.current_closure)
    }

    /// Drop the `parameterize` frames a call pushed above `depth` and left
    /// behind by raising. The error abandoned the activations that pushed
    /// them, so no scope-end `PopParamFrame` will run for them, and the caller
    /// — or a restart of it — runs on with its own bindings
    /// (docs/signals/primitives.md § "Where a restart lands"). A no-op unless
    /// the fiber's signal is an error. Called where a caller learns its callee
    /// raised: a nested interpreted callee (`run_dispatch`), a compiled one
    /// (`call_inner`), and a re-entered body the error abandons
    /// (`execute_bytecode_saving_stack`).
    pub(crate) fn drop_abandoned_param_frames(&mut self, depth: usize) {
        if self
            .fiber
            .signal
            .is_some_and(|(bits, _)| bits.intersects(SIG_ERROR))
        {
            self.fiber.param_frames.truncate(depth);
        }
    }

    /// Open a fresh activation: a region-remap frame, so the body's static
    /// region slots map to fresh physical regions (docs/regions/semantics.md —
    /// every value its own region), and the dues slot beside it.
    ///
    /// A tail call BUILT in compiled code strands its releases on an activation
    /// that pops its own dues slot at the tail-call sentinel, so it leaves them
    /// on `pending_tail_deferrals` for the activation that runs the callee
    /// (docs/impl/region/relocate.md § "A channel built in compiled code hands
    /// its release forward"). That callee's activation opens here, so this is
    /// where the hand-off is collected.
    fn open_activation(&mut self) {
        self.push_activation_region_map();
        if !self.pending_tail_deferrals.is_empty() {
            for region in std::mem::take(&mut self.pending_tail_deferrals) {
                self.activation_dues().defer(region);
            }
        }
    }

    /// Close the activation `open_activation` opened, once its body has ended
    /// with `result`.
    ///
    /// On a suspending exit, MOVE the region remap and what the activation
    /// owes into the result, so a caller that builds a park from it can attach
    /// both (cross-yield remap preservation — docs/impl/region/model.md). A
    /// suspend handler that parked a frame already took the dues (this reads
    /// default); a pause with no frame of its own (fuel) leaves them here
    /// (docs/impl/region/owner.md § "Owner nodes").
    ///
    /// `entry_depth` is how many region-remap frames the fiber held before the
    /// activation opened. Every activation the body entered — interpreted or
    /// compiled — must have handed its own frame back by now, or `last()` names
    /// a callee's leftover map and this activation's slot-routed releases
    /// resolve against the wrong frame (docs/impl/region/rules.md Rule 4).
    fn close_activation(
        &mut self,
        result: &mut ExecResult,
        #[cfg(debug_assertions)] entry_depth: usize,
    ) {
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            self.fiber.activation_region_maps.len(),
            entry_depth + 1,
            "region-remap frames left unbalanced by this activation's body: \
             entered at depth {entry_depth}, returned at depth {} (one exit path \
             pushed without popping)",
            self.fiber.activation_region_maps.len(),
        );
        if !result.bits.is_empty() {
            result.activation_region_map = self
                .fiber
                .activation_region_maps
                .last()
                .cloned()
                .unwrap_or_default();
            result.activation_dues = self.take_activation_dues();
        }
        self.pop_activation_region_map();
    }

    /// The tail-call trampoline shared by `execute_bytecode_from_ip` and
    /// `execute_bytecode_saving_stack`: run the activation from `start_ip`,
    /// replace its body at each tail call, and close it out when it ends.
    /// `walk_abandoned` is `end_activation`'s.
    fn trampoline_loop(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
        walk_abandoned: bool,
    ) -> ExecResult {
        let mut current_code = code.clone();
        let mut current_env = closure_env.clone();
        let mut current_ip = start_ip;
        let mut accumulated_squelch_mask = SignalBits::EMPTY;

        loop {
            let exit = self.run_dispatch(&current_code, &current_env, current_ip);
            if exit.bits.is_empty() {
                if let Some(tail) = self.pending_tail_call.take() {
                    accumulated_squelch_mask |= tail.squelch_mask;
                    (current_code, current_env) = self.replace_by_tail_call(tail);
                    current_ip = 0;
                    continue;
                }
            }
            break self.end_activation(
                current_code,
                current_env,
                exit,
                accumulated_squelch_mask,
                walk_abandoned,
            );
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
        // The caller (`replay_suspended`) owns this frame and may re-park it, so
        // its error exit is not an abandonment the walk may act on.
        self.trampoline_loop(code, closure_env, start_ip, false)
    }

    /// Run a body from IP 0 on the current fiber as a fresh activation,
    /// answering how it ended. The result value is stored in
    /// `self.fiber.signal`.
    ///
    /// Saves/restores the caller's stack around execution.
    /// Handles pending tail calls in a loop.
    pub(crate) fn execute_bytecode_saving_stack(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
    ) -> ExecResult {
        let saved_stack = std::mem::take(&mut self.fiber.stack);
        // Install the executing-closure register for this activation. The
        // entrant sets the one-shot `pending_entry_closure` immediately before
        // this call; take it (resetting to NIL) and save the caller's register
        // to restore on return. A caller that set nothing enters NIL — the body
        // runs untracked. The trampoline re-installs it on each tail-call frame
        // replacement; on a suspending exit `result.current_closure` carries the
        // value at suspend for the caller to park.
        let saved_closure = self.fiber.current_closure;
        let entering = std::mem::replace(&mut self.pending_entry_closure, Value::NIL);
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(entering, code);
        self.fiber.current_closure = entering;
        // Whether THIS activation's frame is parked on an error exit is the
        // entrant's to say, and only `do_fiber_first_resume` says yes; taking the
        // one-shot here leaves every body this one calls answering no
        // (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
        // it still owes").
        let parks_error_frame = std::mem::take(&mut self.pending_error_park);
        let param_depth = self.fiber.param_frames.len();
        #[cfg(debug_assertions)]
        let entry_depth = self.fiber.activation_region_maps.len();
        self.open_activation();
        // A re-entry nests on the Rust stack. Refusing it while the reserve is
        // gone turns a thread overflow into a halt the program can report; the
        // activation is opened first so the halt leaves it the way any other
        // halted body does.
        let mut result = if crate::vm::native_stack::below(crate::vm::native_stack::REENTRY_RESERVE)
        {
            self.halt_native_stack_exhausted();
            self.end_activation(
                code.clone(),
                closure_env.clone(),
                Exit::at(SIG_HALT, 0),
                SignalBits::EMPTY,
                !parks_error_frame,
            )
        } else {
            self.trampoline_loop(code, closure_env, 0, !parks_error_frame)
        };
        self.close_activation(
            &mut result,
            #[cfg(debug_assertions)]
            entry_depth,
        );
        // A body parked for a restart keeps its bindings; one the error
        // abandons leaves them behind.
        if !parks_error_frame {
            self.drop_abandoned_param_frames(param_depth);
        }
        // Restore the caller's executing-closure register. On a suspending exit the
        // callee's value is already in `result.current_closure`; on normal return
        // the caller resumes as itself.
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
    /// `SIG_SWITCH`-draining loop the root dispatch ([`VM::execute_proto`])
    /// runs, so a `fiber/resume` inside the thunk completes inside the caller's
    /// scope. The module doc's SIG_SWITCH section says why that matters.
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
