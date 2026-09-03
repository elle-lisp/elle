//! `compile/run-on :bytecode` — force pure interpreter execution.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};
use crate::vm::core::VM;

impl VM {
    /// Run a closure under pure bytecode interpretation.
    ///
    /// Saves and restores JIT policy so the VM's tier dispatch can't
    /// route the top-level call through JIT or MLIR. Nested calls still
    /// honor the surrounding configuration — Phase 1 differential tests
    /// use leaf functions, so this is a non-issue in practice.
    pub fn invoke_closure_bytecode(
        &mut self,
        closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        // Arity check.
        if !self.check_arity(&closure.template.arity(), args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        // Build environment.
        let new_env = match self.build_closure_env(closure, args) {
            Some(env) => env,
            None => {
                return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            }
        };

        let saved_jit = self.runtime_config.jit.clone();
        self.runtime_config.jit = crate::config::JitPolicy::Off;

        let squelch_mask = closure.squelch_mask;

        // Hand the target its executing-closure register via the one-shot — a
        // forced-tier entry runs a closure body like any other entrant.
        self.pending_entry_closure = closure_val;
        let result = self.execute_bytecode_saving_stack(&closure.template.code(), &new_env);

        self.runtime_config.jit = saved_jit;

        let bits = result.bits;
        if bits.is_empty() {
            let val = if let Some((_, v)) = self.fiber.signal.take() {
                v
            } else {
                Value::NIL
            };
            return (SIG_OK, val);
        } else if bits == crate::value::SIG_HALT {
            // (halt) → NIL → absorb as success. (halt <value>) → propagate.
            let val = if let Some((_, v)) = self.fiber.signal.take() {
                v
            } else {
                Value::NIL
            };
            if val == Value::NIL {
                return (SIG_OK, val);
            }
            return (crate::value::SIG_HALT, val);
        }

        if self.enforce_squelch(bits, squelch_mask) {
            return self.fiber.signal.take().unwrap();
        }

        // A suspend-class signal leaves this forced-tier entry as a value; the
        // park it names is abandoned with its host (`abandon_hosted_park`).
        self.abandon_hosted_park(bits);

        // Other errors: extract from fiber signal.
        if let Some((sig_bits, val)) = self.fiber.signal.take() {
            return (sig_bits, val);
        }
        (
            SIG_ERROR,
            self.escaping_error("runtime-error", "unexpected signal"),
        )
    }
}
