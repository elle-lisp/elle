// audited: 2026-11-12
//! The tail-call trampoline: how an activation runs each pending tail callee in place of its body.
//!
//! docs/impl/vm.md
//! docs/impl/region/relocate.md

use super::{ExecResult, Exit};
use crate::value::fiber::TailBoundary;
use crate::value::Value;
use crate::vm::core::VM;
use std::rc::Rc;

impl VM {
    /// Drop the operand stack to the frame base before a tail callee replaces
    /// the frame.
    ///
    /// The fresh-frame invariant (docs/impl/region/rules.md): the
    /// callee's unwritten local slots must read NIL exactly as on a fresh
    /// activation — a branch-arm temp's scope-end release reads its slot
    /// unconditionally and no-ops only on NIL. The reused stack still holds
    /// the caller's locals at those indices (all dead: released at last use or
    /// moved into the callee), so drop them before the callee runs
    /// (runtime::tests::ownership::frame). Every driver drops them, the root
    /// driver `execute_code` included.
    pub(in crate::vm) fn drop_replaced_locals(&mut self) {
        self.fiber.stack.truncate(self.current_frame_base());
    }

    /// Replace the running activation's body with a pending tail call's callee
    /// and run it from its first instruction with `run`, the dispatch loop the
    /// consumer drives. Answers the callee's code and environment, which the
    /// activation now runs, and the exit `run` stopped at.
    ///
    /// Every consumer of `pending_tail_call` runs the callee here. The caller
    /// adds the tail call's boundary to the activation's [`TailBoundary`], and
    /// drops the replaced locals first ([`Self::drop_replaced_locals`]).
    pub(in crate::vm) fn run_tail_callee(
        &mut self,
        tail: crate::vm::core::TailCallInfo,
        run: fn(&mut VM, &crate::value::Code, &Rc<Vec<Value>>, usize) -> Exit,
    ) -> (crate::value::Code, Rc<Vec<Value>>, Exit) {
        self.clear_region_map_for_tail_callee();
        let (code, env) = tail.body();
        // The frame is reused in place but now runs the tail callee: track it as
        // the executing closure so a self-edge resolved after this replacement
        // names the right closure (a self-recursive `loop` re-installs itself; a
        // tail call to a sibling installs the sibling).
        #[cfg(debug_assertions)]
        Self::debug_assert_entry_closure_matches(tail.closure(), &code);
        self.fiber.current_closure = tail.closure();
        let exit = run(self, &code, &env, 0);
        (code, env, exit)
    }

    /// Run an activation's body from `start_ip`, then hand it to the
    /// tail-call trampoline. `execute_bytecode_from_ip` and
    /// `execute_bytecode_saving_stack` enter here.
    pub(super) fn trampoline_at(
        &mut self,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> ExecResult {
        let tail_boundary = TailBoundary::none(self.fiber.param_depth());
        let exit = self.run_dispatch(code, closure_env, start_ip);
        self.trampoline_loop(code.clone(), closure_env.clone(), exit, tail_boundary)
    }

    /// The trampoline for an activation whose first body is a pending tail
    /// call's callee. The caller owns the tail call's own boundary.
    #[cfg(feature = "jit")]
    pub(super) fn trampoline_tail(&mut self, tail: crate::vm::core::TailCallInfo) -> ExecResult {
        let tail_boundary = TailBoundary::none(self.fiber.param_depth());
        self.drop_replaced_locals();
        let (code, env, exit) = self.run_tail_callee(tail, Self::run_dispatch);
        self.trampoline_loop(code, env, exit, tail_boundary)
    }

    /// The tail-call trampoline: continue an activation whose body `code`
    /// exited at `exit`, run each pending tail callee in its place, and close
    /// the activation out when it ends.
    fn trampoline_loop(
        &mut self,
        mut code: crate::value::Code,
        mut env: Rc<Vec<Value>>,
        mut exit: Exit,
        mut tail_boundary: TailBoundary,
    ) -> ExecResult {
        loop {
            if exit.bits.is_empty() {
                if let Some(mut tail) = self.pending_tail_call.take() {
                    tail_boundary.add(tail.take_boundary(), self.fiber.param_depth());
                    self.drop_replaced_locals();
                    (code, env, exit) = self.run_tail_callee(tail, Self::run_dispatch);
                    continue;
                }
            }
            break self.end_activation(code, env, exit, tail_boundary);
        }
    }
}
