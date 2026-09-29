// audited: 2026-09-29
//! `compile/run-on :jit` — force Cranelift JIT execution.
//!
//! Both variants live here: the real entry point under `--features jit`, and
//! the always-rejecting stub when the feature is off, so callers can invoke
//! `invoke_closure_jit` unconditionally.
//!
//! docs/impl/jit.md
//! docs/impl/region/park.md

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
        let mut lir = match closure.template.lir_function() {
            Some(l) => (**l).clone(),
            None => return (SIG_ERROR, rejected(self, "jit", "closure has no LIR")),
        };
        // Backfill a nameless LIR from the template so the compile records a
        // readable code-address registry entry (docs/impl/jit.md § "The
        // code-address registry").
        if lir.name.is_none() {
            lir.name = Some(closure.template.display_label());
        }

        // Arity check writes to fiber.signal on mismatch.
        if !self.check_arity(&closure.template.arity(), args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        // Use the cached JIT code if available, else force-compile.
        let bytecode_ptr = closure.template.bytecode().as_ptr();
        let jit_code = match self.jit_code_for(bytecode_ptr) {
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
                match compiler.compile(&lir, Vec::new()) {
                    Ok(jc) => {
                        let jc = Arc::new(jc);
                        self.install_jit_code((*closure.template).clone(), jc.clone());
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
        // Compiled frames that leave by an error or a refused park pop none of
        // the `parameterize` frames they pushed, so each such exit below
        // truncates to this.
        let depth = self.fiber.param_depth();

        let result_jv = self.call_jit(&jit_code, closure, args, closure_val);

        // Capture any signal the JIT set (errors, halts, yields).
        let post_signal = self.fiber.signal.take();
        // An error abandons the compiled frames. A tail callee that raises
        // drops its own, in `execute_bytecode_saving_stack`.
        if post_signal.is_some_and(|(bits, _)| bits.intersects(SIG_ERROR)) {
            self.fiber.unwind_params(depth);
        }

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

                // The tail callee's own signal — its result, its error, or the
                // park it made — taken before the caller's is restored, as the
                // non-tail path below takes `post_signal`.
                let tail_signal = self.fiber.signal.take();
                self.fiber.stack = saved_stack;
                self.fiber.signal = saved_signal;

                if eb.is_empty() {
                    let val = tail_signal.map_or(Value::NIL, |(_, v)| v);
                    return (SIG_OK, val);
                } else if eb == crate::value::SIG_HALT {
                    let val = tail_signal.map_or(Value::NIL, |(_, v)| v);
                    if val == Value::NIL {
                        return (SIG_OK, val);
                    }
                    return (crate::value::SIG_HALT, val);
                } else if eb.intersects(SIG_ERROR) {
                    if let Some((bits, val)) = tail_signal {
                        return (bits, val);
                    }
                    return (
                        SIG_ERROR,
                        self.escaping_error("runtime-error", "tail-call error"),
                    );
                } else {
                    // Suspending signal — not supported under compile/run-on.
                    // This host refuses the park and raises at its own call.
                    self.refuse_held_park(eb, tail_signal, depth);
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

        // An error or halt wins over the sentinel. Compiled code leaves an
        // `(error …)` through the yield side exit, as it leaves any emit, so
        // the sentinel alone does not say the closure suspended.
        if let Some((bits, val)) = post_signal {
            if bits.intersects(SIG_ERROR) || bits.intersects(crate::value::SIG_HALT) {
                return (bits, val);
            }
        }

        if result_jv == crate::jit::YIELD_SENTINEL {
            // Squelch enforcement on the suspension the sentinel reports. The
            // signal is on `post_signal`, not `fiber.signal` — the caller's
            // signal is back in place by here — so this asks the shared
            // predicate directly instead of going through `enforce_squelch`.
            let yield_bits = post_signal.map_or(crate::value::SIG_YIELD, |(bits, _)| bits);
            let squelched = crate::signals::squelched_bits(yield_bits, closure.squelch_mask);
            if !squelched.is_empty() {
                // …and the park it ends is `post_signal` for the same reason:
                // `fiber.signal` holds the caller's by here.
                return (
                    SIG_ERROR,
                    self.squelch_violation(squelched, post_signal, depth),
                );
            }

            // Not squelched: this host refuses the park and raises at its own
            // call. The park is `post_signal`, for the reason the squelch check
            // above names.
            self.refuse_held_park(yield_bits, post_signal, depth);

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

        // Any other signal set during execution wins over the return value.
        if let Some((bits, val)) = post_signal {
            // Squelch enforcement for a signal the sentinel did not report —
            // same predicate, same reason for not routing through
            // `enforce_squelch`.
            let squelched = crate::signals::squelched_bits(bits, closure.squelch_mask);
            if !squelched.is_empty() {
                return (
                    SIG_ERROR,
                    self.squelch_violation(squelched, post_signal, depth),
                );
            }
            if !bits.is_empty() {
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
