//! `compile/run-on :jit` — force Cranelift JIT execution.
//!
//! Both variants live here: the real entry point under `--features jit`, and
//! the always-rejecting stub when the feature is off, so callers can invoke
//! `invoke_closure_jit` unconditionally.

use super::rejected;
#[cfg(feature = "jit")]
use crate::value::SIG_OK;
use crate::value::{SignalBits, Value, SIG_ERROR};
use crate::vm::core::VM;
#[cfg(feature = "jit")]
use std::sync::Arc;

impl VM {
    /// Run a closure via Cranelift JIT.
    ///
    /// Force-compiles the closure if it's not already cached; rejects
    /// with `:tier-rejected` if it has no LIR or the JIT compiler refuses.
    #[cfg(feature = "jit")]
    pub fn invoke_closure_jit(
        &mut self,
        closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        // Closure must have LIR — primitives, macros, etc. don't.
        let lir = match &closure.template.lir_function {
            Some(l) => (**l).clone(),
            None => return (SIG_ERROR, rejected(self, "jit", "closure has no LIR")),
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
                            rejected(self, "jit", format!("JIT compiler init failed: {}", e)),
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
                        let jc = Arc::new(jc);
                        self.jit_cache.insert(bytecode_ptr, jc.clone());
                        jc
                    }
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            rejected(self, "jit", format!("JIT rejected closure: {}", e)),
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

        // Decode the return value — handle tail calls before restoring
        // the caller's stack, since the trampoline needs the VM state.

        // Tail-call trampoline: if the JIT ended with a tail call, consume
        // the pending_tail_call and execute the callee via bytecode. This
        // matches the pattern in run_jit (jit_entry.rs) — the tail-call
        // target may be a different closure, so we interpret its bytecode.
        if result_jv == crate::jit::TAIL_CALL_SENTINEL {
            if let Some(tail) = self.pending_tail_call.take() {
                // The resolved body is the tail callee's — hand it its
                // executing-closure register (see `run_jit`'s sentinel arm).
                self.pending_entry_closure = tail.closure;
                let exec_result = self.execute_bytecode_saving_stack(&tail.code, &tail.env);
                let eb = exec_result.bits;

                self.fiber.stack = saved_stack;
                if let Some(sig) = saved_signal {
                    self.fiber.signal = Some(sig);
                }

                if eb.is_ok() {
                    let val = if let Some((_, v)) = self.fiber.signal.take() {
                        v
                    } else {
                        Value::NIL
                    };
                    return (SIG_OK, val);
                } else if eb == crate::value::SIG_HALT {
                    let val = if let Some((_, v)) = self.fiber.signal.take() {
                        v
                    } else {
                        Value::NIL
                    };
                    if val == Value::NIL {
                        return (SIG_OK, val);
                    }
                    return (crate::value::SIG_HALT, val);
                } else if eb.contains(SIG_ERROR) {
                    // Error already set on fiber.signal — extract it.
                    if let Some((bits, val)) = self.fiber.signal.take() {
                        return (bits, val);
                    }
                    return (
                        SIG_ERROR,
                        self.escaping_error("runtime-error", "tail-call error"),
                    );
                } else {
                    // Suspending signal — not supported under compile/run-on.
                    return (
                        SIG_ERROR,
                        rejected(self, "jit", "tail-call target yielded under compile/run-on"),
                    );
                }
            } else {
                self.fiber.stack = saved_stack;
                if let Some(sig) = saved_signal {
                    self.fiber.signal = Some(sig);
                }
                return (
                    SIG_ERROR,
                    rejected(self, "jit", "tail-call sentinel without pending call (bug)"),
                );
            }
        }

        // Restore caller state for non-tail-call paths.
        self.fiber.stack = saved_stack;
        if let Some(sig) = saved_signal {
            self.fiber.signal = Some(sig);
        }

        if result_jv == crate::jit::YIELD_SENTINEL {
            // Squelch enforcement: if the closure has a squelch mask
            // covering the yield signal, produce :signal-violation.
            let squelch_mask = closure.squelch_mask;
            if !squelch_mask.is_empty() {
                let yield_bits = if let Some((bits, _)) = &post_signal {
                    *bits
                } else {
                    crate::value::SIG_YIELD
                };
                let squelched = yield_bits.intersection(squelch_mask);
                if !squelched.is_empty() {
                    let squelched_str = {
                        let registry = crate::signals::registry::global_registry().lock().unwrap();
                        registry.format_signal_bits(squelched)
                    };
                    self.discard_suspended_frames();
                    let err = self.escaping_error(
                        "signal-violation",
                        format!("squelch: signal {} caught at boundary", squelched_str),
                    );
                    return (SIG_ERROR, err);
                }
            }

            if let Some((bits, val)) = post_signal {
                return (
                    SIG_ERROR,
                    rejected(
                        self,
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
                rejected(self, "jit", "closure yielded under compile/run-on"),
            );
        }

        // Error or halt set during execution wins over the return value.
        if let Some((bits, val)) = post_signal {
            // Squelch enforcement for non-yield signals.
            let squelch_mask = closure.squelch_mask;
            if !squelch_mask.is_empty()
                && !bits.contains(SIG_ERROR)
                && !bits.contains(crate::value::SIG_HALT)
            {
                let squelched = bits.intersection(squelch_mask);
                if !squelched.is_empty() {
                    let squelched_str = {
                        let registry = crate::signals::registry::global_registry().lock().unwrap();
                        registry.format_signal_bits(squelched)
                    };
                    self.discard_suspended_frames();
                    let err = self.escaping_error(
                        "signal-violation",
                        format!("squelch: signal {} caught at boundary", squelched_str),
                    );
                    return (SIG_ERROR, err);
                }
            }
            if !bits.is_ok() {
                return (bits, val);
            }
        }

        (SIG_OK, result_jv.to_value())
    }

    /// Stub when JIT feature is disabled — always rejects with `:tier-rejected`.
    #[cfg(not(feature = "jit"))]
    pub fn invoke_closure_jit(
        &mut self,
        _closure_val: Value,
        _closure: &crate::value::Closure,
        _args: &[Value],
    ) -> (SignalBits, Value) {
        (
            SIG_ERROR,
            rejected(self, "jit", "JIT feature not compiled in"),
        )
    }
}
