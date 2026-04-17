//! Force-dispatch a closure on a specific compilation tier.
//!
//! Powers the `compile/run-on` primitive used by `lib/differential.lisp`
//! to verify that the same closure produces the same result on every
//! tier that accepts it.
//!
//! Tiers:
//! - `:bytecode` — pure interpreter (this closure's code is interpreted;
//!   nested calls still go through normal tier dispatch)
//! - `:jit` — force-compiles via Cranelift, then dispatches to native code
//! - `:mlir-cpu` — force-compiles via MLIR + LLVM, dispatches via the
//!   `MlirCache` (only available with `--features mlir`)
//!
//! Each entry point returns `(SignalBits, Value)`. Tier ineligibility
//! surfaces as a structured `:tier-rejected` error so callers can skip
//! the tier rather than failing.

use crate::value::{error_val_extra, SignalBits, Value, SIG_ERROR, SIG_OK};
use std::rc::Rc;

use super::core::VM;

/// Build a structured `:tier-rejected` error.
fn rejected(tier: &str, msg: impl Into<String>) -> Value {
    error_val_extra(
        "tier-rejected",
        msg,
        &[
            ("tier", Value::keyword(tier)),
            ("reason", Value::keyword("ineligible")),
        ],
    )
}

impl VM {
    /// Run a closure under pure bytecode interpretation.
    ///
    /// Saves and restores `jit_enabled` so the VM's tier dispatch can't
    /// route the top-level call through JIT or MLIR. Nested calls still
    /// honor the surrounding configuration — Phase 1 differential tests
    /// use leaf functions, so this is a non-issue in practice.
    pub fn invoke_closure_bytecode(
        &mut self,
        _closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        let saved_jit = self.jit_enabled;
        self.jit_enabled = false;

        let result = self.call_closure(closure, args);

        self.jit_enabled = saved_jit;

        match result {
            Ok(v) => (SIG_OK, v),
            Err(msg) => (SIG_ERROR, crate::value::error_val("runtime-error", msg)),
        }
    }

    /// Run a closure via Cranelift JIT.
    ///
    /// Force-compiles the closure if it's not already cached; rejects
    /// with `:tier-rejected` if it has no LIR or the JIT compiler refuses.
    pub fn invoke_closure_jit(
        &mut self,
        closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        // Closure must have LIR — primitives, macros, etc. don't.
        let lir = match closure.template.lir_function.clone() {
            Some(l) => l,
            None => return (SIG_ERROR, rejected("jit", "closure has no LIR")),
        };

        // Arity check writes to fiber.signal on mismatch.
        if !self.check_arity(&closure.template.arity, args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        // Use the cached JIT code if available, else force-compile.
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let jit_code = match self.jit_cache.get(&bytecode_ptr).cloned() {
            Some(jc) => jc,
            None => {
                let compiler = match crate::jit::JitCompiler::new() {
                    Ok(c) => c,
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            rejected("jit", format!("JIT compiler init failed: {}", e)),
                        )
                    }
                };
                match compiler.compile(
                    &lir,
                    None,
                    (*closure.template.symbol_names).clone(),
                    Vec::new(),
                ) {
                    Ok(jc) => {
                        let jc = Rc::new(jc);
                        self.jit_cache.insert(bytecode_ptr, jc.clone());
                        jc
                    }
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            rejected("jit", format!("JIT rejected closure: {}", e)),
                        )
                    }
                }
            }
        };

        // Save the operand stack and signal — call_jit may push and set.
        let saved_stack = std::mem::take(&mut self.fiber.stack);
        let saved_signal = self.fiber.signal.take();

        let result_jv = self.call_jit(&jit_code, closure, args, closure_val);

        // Capture any signal the JIT set (errors, halts, yields).
        let post_signal = self.fiber.signal.take();
        self.fiber.stack = saved_stack;
        if let Some(sig) = saved_signal {
            self.fiber.signal = Some(sig);
        }

        // Decode the return value.
        if result_jv == crate::jit::YIELD_SENTINEL {
            // The JIT yielded — Phase 1 doesn't support resuming through this path.
            if let Some((bits, val)) = post_signal {
                return (
                    SIG_ERROR,
                    rejected(
                        "jit",
                        format!(
                            "closure yielded under compile/run-on (signal {}, value type {})",
                            bits,
                            val.type_name()
                        ),
                    ),
                );
            }
            return (
                SIG_ERROR,
                rejected("jit", "closure yielded under compile/run-on"),
            );
        }

        if result_jv == crate::jit::TAIL_CALL_SENTINEL {
            // Pending tail call — Phase 1 doesn't drive the tail-call loop here.
            self.pending_tail_call.take();
            return (
                SIG_ERROR,
                rejected(
                    "jit",
                    "closure ended in a tail call; not yet supported under compile/run-on",
                ),
            );
        }

        // Error or halt set during execution wins over the return value.
        if let Some((bits, val)) = post_signal {
            if !bits.is_ok() {
                return (bits, val);
            }
        }

        (SIG_OK, result_jv.to_value())
    }

    /// Run a closure via the MLIR/LLVM CPU tier-2 backend.
    ///
    /// Requires `--features mlir`. The closure must satisfy the
    /// `is_mlir_cpu_eligible` predicate (no captures, exact arity, only
    /// arithmetic/comparison/local instructions, integer-typed return)
    /// and all arguments must be integers — MLIR sees a flat i64 world.
    #[cfg(feature = "mlir")]
    pub fn invoke_closure_mlir_cpu(
        &mut self,
        _closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        let lir = match closure.template.lir_function.as_ref() {
            Some(l) => l.clone(),
            None => return (SIG_ERROR, rejected("mlir-cpu", "closure has no LIR")),
        };

        if !lir.is_mlir_cpu_eligible() {
            return (
                SIG_ERROR,
                rejected("mlir-cpu", "closure is not MLIR-CPU eligible"),
            );
        }

        if !self.check_arity(&closure.template.arity, args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        // Unbox to i64. Non-int args fall through with a structured error
        // (same contract as try_mlir_call).
        let mut int_args: Vec<i64> = Vec::with_capacity(args.len());
        for (i, v) in args.iter().enumerate() {
            match v.as_int() {
                Some(n) => int_args.push(n),
                None => {
                    return (
                        SIG_ERROR,
                        rejected(
                            "mlir-cpu",
                            format!(
                                "arg {} is {}, not an integer; MLIR-CPU requires i64 args",
                                i,
                                v.type_name()
                            ),
                        ),
                    )
                }
            }
        }

        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let cache = self
            .mlir_cache
            .get_or_insert_with(crate::mlir::MlirCache::new);

        // Ensure compiled.
        if !cache.contains(bytecode_ptr) {
            if let Err(e) = cache.compile(bytecode_ptr, &lir) {
                return (
                    SIG_ERROR,
                    rejected("mlir-cpu", format!("MLIR compilation failed: {}", e)),
                );
            }
        }

        // Reborrow as immutable for call.
        let cache = self.mlir_cache.as_ref().unwrap();
        match cache.call(bytecode_ptr, &int_args) {
            Some(Ok(result)) => (SIG_OK, Value::int(result)),
            Some(Err(e)) => (
                SIG_ERROR,
                crate::value::error_val("mlir-error", format!("MLIR execution failed: {}", e)),
            ),
            None => (
                SIG_ERROR,
                rejected("mlir-cpu", "MLIR cache miss after compile (bug)"),
            ),
        }
    }
}
