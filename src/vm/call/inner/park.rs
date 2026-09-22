// audited: 2026-09-21
//! Rebuilding a non-yield-suspended interpreter callee's inner frame, shared by
//! the interpreter call path and the JIT→interpreter fallback.
//!
//! docs/impl/region/owner.md

use super::*;

impl VM {
    /// Park a non-yield-suspended interpreter callee's inner frame onto `frames`.
    ///
    /// `execute_bytecode_saving_stack` returns a callee interrupted by a
    /// non-yield signal (SIG_FUEL and its compounds) with its live operand stack
    /// in `ExecResult.stack`; it does NOT park it in `fiber.suspended` — only
    /// `handle_yield` does that, and only for SIG_YIELD. So every caller that
    /// runs an interpreter callee through `execute_bytecode_saving_stack` and can
    /// suspend must reconstruct that frame, or the callee's state is lost and
    /// resume injects nil as the call's return value. Shared by the interpreter
    /// call path (`call_inner`) and the JIT→interpreter fallback (`elle_jit_call`),
    /// which otherwise drifted (the JIT twin dropped the frame —
    /// `tests/elle/fuel-jit-preempt.lisp`).
    ///
    /// The `frames.is_empty()` guard leaves a deeper yield's already-parked chain
    /// untouched. `push_resume_value` is false for a fuel pause (the interrupted
    /// opcode re-executes from `ip`, injecting no extra value) and true otherwise.
    /// The callee's region remap and its activation's dues ride out in `result`, moved into
    /// the frame so the remap survives the yield (docs/impl/region/owner.md).
    pub(crate) fn park_suspended_callee_frame(
        &mut self,
        frames: &mut Vec<SuspendedFrame>,
        bits: SignalBits,
        result: crate::vm::execute::ExecResult,
    ) {
        if frames.is_empty() && !result.stack.is_empty() {
            let inner = BytecodeFrame::suspend(
                result.code,
                result.env,
                result.ip,
                result.stack,
                !bits.intersects(SIG_FUEL),
                result.activation_region_map,
                result.activation_dues,
                result.current_closure,
                self.heap(),
            );
            frames.push(SuspendedFrame::Bytecode(inner));
        }
    }
}
