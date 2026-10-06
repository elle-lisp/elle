// audited: 2026-10-06
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
//! ### An error from inner execution
//!
//! An error parks every frame from the raise to the body, the body's own frame
//! included (`VM::park_raised`), so the chain sits in `fiber.suspended` when
//! `execute_bytecode_saving_stack` returns, and the `parameterize` frames the
//! body pushed stay in force. A caller that resumes the body's frames keeps the
//! park: a fiber's first resume, and a compiled or WASM caller that parks behind
//! its callee. Every other host ends it through `VM::discard_hosted_error`
//! before it raises or reports.
//!
//! ### A suspend from inner execution
//!
//! If the inner closure suspends (a yield, an I/O request),
//! `execute_bytecode_saving_stack` returns the suspending bits with the outer
//! stack restored. No host resumes the inner continuation. `eval`, the module
//! loaders and the test-setup loader end the park through
//! `VM::refuse_hosted_park` and raise at their own call. `arena/allocs` returns
//! the signal from its own call, so its park ends there, through
//! `VM::abandon_hosted_park`.
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
//! (`handle_sig_switch`). The top-level dispatch loop (`VM::execute_code`)
//! is that trampoline at the root; a re-entrant boundary that runs a thunk on
//! the current fiber must be one too. If it is not, the `SIG_SWITCH` unwinds
//! straight out of `execute_bytecode_saving_stack` and the thunk's continuation
//! is later resumed by the *outer* trampoline — that is, OUTSIDE the
//! re-entrant caller's scope. An `arena/allocs` measurement would then answer
//! with the resumed child's value instead of `(result . net)` and never finish
//! the thunk (`tests/impl/arena.lisp`, the thunk that resumes a fiber under
//! "arena/allocs"; the `fiber-spawn-10` scenario in `tests/impl/resource.lisp`).
//!
//! `VM::run_thunk_to_completion` is the safe entry point: it drives
//! `SIG_SWITCH` to completion exactly as the root loop does, so a nested
//! `fiber/resume` runs fully and the thunk produces its real result. Prefer it
//! over a raw `execute_bytecode_saving_stack` for any caller that runs a thunk
//! as part of the *current* fiber's execution (`eval`, the module loaders,
//! `arena/allocs`, the test-setup module loader). Do NOT use it when running a
//! *child fiber's* body (`do_fiber_first_resume`): there `SIG_SWITCH` must
//! propagate to the child's own driving `do_fiber_resume`, not be driven here.
//!
//! ### Rules for new callers
//!
//! If you add a new SIG_QUERY handler or primitive that calls a user closure
//! via `execute_bytecode_saving_stack`:
//!
//! 1. Read `fiber.signal` immediately after return to get the result.
//! 2. Check `exec_result.bits` for `SIG_ERROR` and `SIG_HALT` before using
//!    the result. On an error, end the parked chain through
//!    `VM::discard_hosted_error`, unless your caller resumes it.
//! 3. If the closure may suspend, end its park as the section above
//!    describes: refuse it through `VM::refuse_hosted_park`, or hand it on as
//!    your own call's park through `VM::abandon_hosted_park`.
//! 4. Do NOT assume `fiber.signal` is unchanged after the call.
//! 5. The inner execution runs on the SAME fiber — same heap, same
//!    parameter frames. It is not isolated.
//! 6. If the closure may call `fiber/resume`, run it through
//!    `VM::run_thunk_to_completion` (not raw `execute_bytecode_saving_stack`)
//!    so the `SIG_SWITCH` trampoline is driven inside your scope — see the
//!    SIG_SWITCH section above.

use crate::value::fiber::{RaiseSite, TailBoundary};
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};
use std::rc::Rc;

use super::core::VM;

mod exit;
mod nested;
mod park;

pub(crate) use exit::{ExecResult, Exit};

impl VM {
    /// Debug-only: the executing-closure register handed to a body entry must
    /// name that body — its template's bytecode must be the very bytes the
    /// entered `Code` names. Called ONLY where the closure is live by
    /// construction (the entrant just took `code` from it), never on a
    /// restored/parked register — a parked register is a possibly-dead borrow
    /// (the region solver frees a closure value at its last use while its
    /// activation runs on), so dereferencing it is unsound.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_entry_closure_matches(entering: Value, code: &crate::value::Code) {
        if let Some(cl) = entering.as_closure() {
            // Identity is the payload's bytecode address: every header over one
            // payload shares it (docs/impl/region/template.md), so two headers
            // for the same function compare equal however each was built, and
            // two different functions never do.
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
    /// answering the callee's code and environment. The caller adds the tail
    /// call's boundary to the activation's [`TailBoundary`].
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
    /// On a signal: the boundary the activation's tail calls accumulated may
    /// refuse it and end the activation with an error
    /// ([`Self::end_refused_activation`]), and otherwise the operand stack
    /// leaves in the result for the code that parks the frame. An error
    /// abandons nothing, so no release runs here (docs/impl/region/unwind.md).
    /// On a clean return: the activation discharges what it owes.
    fn end_activation(
        &mut self,
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        exit: Exit,
        tail_boundary: TailBoundary,
    ) -> ExecResult {
        if exit.bits.is_empty() {
            // Normal completion: discharge what this activation owes — the
            // decrefs its frame-replacing tail calls left dead, and its owner
            // node, whose single decref subtree-drops every member the
            // activation adopted (docs/impl/region/owner.md).
            // One clean-break discipline for both: a frame-replacing tail call
            // keeps the activation alive to the recursion's completion here,
            // and so keeps everything it owes.
            self.release_activation_dues();
            return ExecResult::ended(exit, code, env, vec![], self.fiber.current_closure);
        }
        let parked = self.fiber.suspended.is_some();
        if let Some(bits) =
            self.enforce_boundary(exit.bits, &tail_boundary.boundary, tail_boundary.depth)
        {
            return self.end_refused_activation(code, env, bits, exit.ip, parked);
        }
        let stack = std::mem::take(&mut self.fiber.stack).into_vec();
        ExecResult::ended(exit, code, env, stack, self.fiber.current_closure)
    }

    /// Close out an activation whose tail call a boundary refused at `ip`, with
    /// the error `bits` the boundary raised, at [`RaiseSite::TailRefused`].
    /// `parked` says whether the refused code parked the activation's frame
    /// (docs/impl/region/unwind.md).
    fn end_refused_activation(
        &mut self,
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        bits: SignalBits,
        ip: usize,
        parked: bool,
    ) -> ExecResult {
        if !parked {
            let payload = self.fiber.signal.map(|(_, v)| v).unwrap_or(Value::NIL);
            self.release_abandoned_frame(&code, payload);
        }
        self.release_activation_dues();
        self.fiber.stack.clear();
        let exit = Exit::raised(bits, ip, RaiseSite::TailRefused);
        ExecResult::ended(exit, code, env, vec![], self.fiber.current_closure)
    }

    /// Open a fresh activation: a region-remap frame, so the body's static
    /// region slots map to fresh physical regions (docs/regions/semantics.md —
    /// every value its own region), and the dues slot beside it.
    ///
    /// A tail call BUILT in compiled code strands its releases on an activation
    /// that pops its own dues slot at the tail-call sentinel, so it leaves them
    /// on `pending_tail_deferrals` for the activation that runs the callee
    /// (docs/impl/region/relocate.md). That callee's activation opens here, so
    /// this is where the hand-off is collected.
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
    /// (docs/impl/region/owner.md).
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
    fn trampoline_loop(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> ExecResult {
        let mut current_code = code.clone();
        let mut current_env = closure_env.clone();
        let mut current_ip = start_ip;
        let mut tail_boundary = TailBoundary::none(self.fiber.param_depth());

        loop {
            let exit = self.run_dispatch(&current_code, &current_env, current_ip);
            if exit.bits.is_empty() {
                if let Some(mut tail) = self.pending_tail_call.take() {
                    tail_boundary.add(std::mem::take(&mut tail.boundary), self.fiber.param_depth());
                    (current_code, current_env) = self.replace_by_tail_call(tail);
                    current_ip = 0;
                    continue;
                }
            }
            break self.end_activation(current_code, current_env, exit, tail_boundary);
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

    /// Run a body from IP 0 on the current fiber as a fresh activation,
    /// answering how it ended. The result value is stored in
    /// `self.fiber.signal`. On an error the body's chain is parked in
    /// `fiber.suspended`, for the caller to keep or discard (module doc).
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
        let param_depth = self.fiber.param_depth();
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
                TailBoundary::none(param_depth),
            )
        } else {
            self.trampoline_loop(code, closure_env, 0)
        };
        self.close_activation(
            &mut result,
            #[cfg(debug_assertions)]
            entry_depth,
        );
        if result.bits.intersects(SIG_ERROR) && !result.bits.intersects(SIG_HALT) {
            self.park_raised(&mut result);
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
    /// callers whose thunk is part of *this* fiber's execution (`eval`, the
    /// module loaders, `arena/allocs`, the test-setup module loader).
    ///
    /// It wraps [`Self::execute_bytecode_saving_stack`] with the same
    /// `SIG_SWITCH`-draining loop the root dispatch ([`VM::execute_code`])
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
        let bits = self.execute_bytecode_saving_stack(code, closure_env).bits;
        self.drive_switches(bits)
    }

    /// Resume a parked chain on the CURRENT fiber, driving the fiber-resume
    /// trampoline as [`Self::run_thunk_to_completion`] does. The WASM tier
    /// resumes a bytecode callee's chain this way, on the VM's fiber where the
    /// chain parked. The result value is left in `self.fiber.signal`.
    #[cfg(feature = "wasm")]
    pub(crate) fn resume_to_completion(
        &mut self,
        frames: Vec<crate::value::SuspendedFrame>,
        resume_value: Value,
    ) -> SignalBits {
        let bits = self.resume_suspended(frames, resume_value);
        self.drive_switches(bits)
    }

    /// Drive the `SIG_SWITCH` trampoline until the code it runs leaves by
    /// another signal.
    fn drive_switches(&mut self, mut bits: SignalBits) -> SignalBits {
        while bits == crate::value::SIG_SWITCH {
            bits = self.handle_sig_switch();
        }
        bits
    }
}
