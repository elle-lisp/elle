// audited: 2026-09-23
//! The two limits on how deep calls nest: the fiber's depth cap, and the
//! native stack a re-entry needs.
//!
//! docs/impl/vm.md

use super::*;

impl VM {
    /// Count one more non-tail closure call in progress on this fiber.
    ///
    /// Past `(vm/config :max-depth)` the count is undone, the fiber carries the
    /// `:stack-overflow` halt, and the answer is `false`: the caller must not
    /// enter the callee. A halt, not an error, because running out is resource
    /// exhaustion no handler can repair, so it passes every signal mask on its
    /// way to the top level.
    pub(crate) fn enter_call_depth(&mut self) -> bool {
        self.fiber.call_depth += 1;
        let max_depth = self.runtime_config.max_depth;
        if self.fiber.call_depth <= max_depth {
            return true;
        }
        self.fiber.call_depth -= 1;
        let err = self.escaping_error(
            "stack-overflow",
            format!("call depth exceeded maximum ({max_depth})"),
        );
        self.fiber.signal = Some((SIG_HALT, err));
        false
    }

    /// Set the `:stack-overflow` halt a re-entry raises when the thread's
    /// native stack is too low to nest another activation on it.
    pub(crate) fn halt_native_stack_exhausted(&mut self) {
        let left = crate::vm::native_stack::remaining().unwrap_or(0);
        let err = self.escaping_error(
            "stack-overflow",
            format!(
                "native stack exhausted: {left} bytes left, below the {} a call \
                 through a primitive or compiled code needs",
                crate::vm::native_stack::REENTRY_RESERVE
            ),
        );
        self.fiber.signal = Some((SIG_HALT, err));
    }
}
