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
#[cfg(feature = "wasm")]
use std::rc::Rc;
#[cfg(feature = "jit")]
use std::sync::Arc;

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
        // Arity check.
        if !self.check_arity(&closure.template.arity, args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        // Build environment.
        let new_env = match self.build_closure_env(closure, args) {
            Some(env) => env,
            None => {
                return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            }
        };

        let saved_jit = self.jit_enabled;
        self.jit_enabled = false;

        let squelch_mask = closure.squelch_mask;

        let result = self.execute_bytecode_saving_stack(
            &closure.template.bytecode,
            &closure.template.constants,
            &new_env,
            &closure.template.location_map,
        );

        self.jit_enabled = saved_jit;

        let bits = result.bits;
        if bits.is_ok() || bits == crate::value::SIG_HALT {
            let val = if let Some((_, v)) = self.fiber.signal.take() {
                v
            } else {
                Value::NIL
            };
            return (SIG_OK, val);
        }

        // Squelch enforcement: if the closure has a squelch mask and a
        // non-error signal matches, convert to :signal-violation.
        if !squelch_mask.is_empty()
            && !bits.contains(SIG_ERROR)
            && !bits.contains(crate::value::SIG_HALT)
        {
            let squelched = bits.intersection(squelch_mask);
            if !squelched.is_empty() {
                let squelched_str = crate::signals::registry::with_registry(|reg| {
                    reg.format_signal_bits(squelched)
                });
                self.fiber.suspended = None;
                self.fiber.signal = None;
                return (
                    SIG_ERROR,
                    error_val_extra(
                        "signal-violation",
                        format!("squelch: signal {} caught at boundary", squelched_str),
                        &[],
                    ),
                );
            }
        }

        // Other errors: extract from fiber signal.
        if let Some((sig_bits, val)) = self.fiber.signal.take() {
            return (sig_bits, val);
        }
        (
            SIG_ERROR,
            crate::value::error_val("runtime-error", "unexpected signal"),
        )
    }

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
                        let jc = Arc::new(jc);
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

        // Decode the return value — handle tail calls before restoring
        // the caller's stack, since the trampoline needs the VM state.

        // Tail-call trampoline: if the JIT ended with a tail call, consume
        // the pending_tail_call and execute the callee via bytecode. This
        // matches the pattern in run_jit (jit_entry.rs) — the tail-call
        // target may be a different closure, so we interpret its bytecode.
        if result_jv == crate::jit::TAIL_CALL_SENTINEL {
            if let Some(tail) = self.pending.take_tail_call() {
                let exec_result = self.execute_bytecode_saving_stack(
                    &tail.bytecode,
                    &tail.constants,
                    &tail.env,
                    &tail.location_map,
                );
                let eb = exec_result.bits;

                self.fiber.stack = saved_stack;
                if let Some(sig) = saved_signal {
                    self.fiber.signal = Some(sig);
                }

                if eb.is_ok() || eb == crate::value::SIG_HALT {
                    // Success — the result is on the fiber signal (set by
                    // execute_bytecode_saving_stack's Halt handler).
                    let val = if let Some((_, v)) = self.fiber.signal.take() {
                        v
                    } else {
                        Value::NIL
                    };
                    return (SIG_OK, val);
                } else if eb.contains(SIG_ERROR) {
                    // Error already set on fiber.signal — extract it.
                    if let Some((bits, val)) = self.fiber.signal.take() {
                        return (bits, val);
                    }
                    return (
                        SIG_ERROR,
                        crate::value::error_val("runtime-error", "tail-call error"),
                    );
                } else {
                    // Suspending signal — not supported under compile/run-on.
                    return (
                        SIG_ERROR,
                        rejected("jit", "tail-call target yielded under compile/run-on"),
                    );
                }
            } else {
                self.fiber.stack = saved_stack;
                if let Some(sig) = saved_signal {
                    self.fiber.signal = Some(sig);
                }
                return (
                    SIG_ERROR,
                    rejected("jit", "tail-call sentinel without pending call (bug)"),
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
                    let squelched_str = crate::signals::registry::with_registry(|reg| {
                        reg.format_signal_bits(squelched)
                    });
                    self.fiber.suspended = None;
                    return (
                        SIG_ERROR,
                        error_val_extra(
                            "signal-violation",
                            format!("squelch: signal {} caught at boundary", squelched_str),
                            &[],
                        ),
                    );
                }
            }

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
                    let squelched_str = crate::signals::registry::with_registry(|reg| {
                        reg.format_signal_bits(squelched)
                    });
                    self.fiber.suspended = None;
                    return (
                        SIG_ERROR,
                        error_val_extra(
                            "signal-violation",
                            format!("squelch: signal {} caught at boundary", squelched_str),
                            &[],
                        ),
                    );
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
        (SIG_ERROR, rejected("jit", "JIT feature not compiled in"))
    }

    /// Run a closure via the WASM backend (Wasmtime tiered compilation).
    ///
    /// Requires `--features wasm`. Force-compiles the closure to a
    /// per-closure WASM module and dispatches through Wasmtime.
    #[cfg(feature = "wasm")]
    pub fn invoke_closure_wasm(
        &mut self,
        _closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        let lir = match closure.template.lir_function.clone() {
            Some(l) => l,
            None => return (SIG_ERROR, rejected("wasm", "closure has no LIR")),
        };

        if !self.check_arity(&closure.template.arity, args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        let bytecode_ptr = closure.template.bytecode.as_ptr();

        // The WASM tiered backend panics on closures with tail calls.
        // TailCall is an LirInstr, not a Terminator.
        let has_tail = lir.blocks.iter().any(|b| {
            b.instructions.iter().any(|si| {
                matches!(
                    si.instr,
                    crate::lir::LirInstr::TailCall { .. }
                        | crate::lir::LirInstr::TailCallArrayMut { .. }
                )
            })
        });
        if has_tail {
            return (
                SIG_ERROR,
                rejected(
                    "wasm",
                    "closure uses tail calls (not supported by WASM tiered mode)",
                ),
            );
        }

        // Use the existing WasmTier if present, else create a temporary one.
        // We must NOT leave a newly-created WasmTier on the VM — the regular
        // dispatch path would start using it for stdlib closures that the
        // tiered WASM backend can't handle.
        let had_tier = self.wasm_tier.is_some();
        if !had_tier {
            match crate::wasm::lazy::WasmTier::new() {
                Ok(tier) => self.wasm_tier = Some(tier),
                Err(e) => {
                    return (
                        SIG_ERROR,
                        rejected("wasm", format!("WasmTier init failed: {}", e)),
                    )
                }
            }
        }

        // Force-compile if not already cached.
        let tier = self.wasm_tier.as_mut().unwrap();
        if !tier.is_compiled(bytecode_ptr) && !tier.compile(bytecode_ptr, &lir) {
            // Remove the temporary tier before returning.
            if !had_tier {
                self.wasm_tier = None;
            }
            return (
                SIG_ERROR,
                rejected("wasm", "WASM compilation rejected this closure"),
            );
        }

        // Call through Wasmtime. The result comes back as (Value, SignalBits).
        let closure_rc = Rc::new(closure.clone());
        let vm_ptr = self as *mut VM;
        let tier = self.wasm_tier.as_ref().unwrap();

        let result = match tier.call(vm_ptr, bytecode_ptr, &closure_rc, args) {
            Ok((value, signal)) => {
                if signal.is_ok() || signal == crate::value::SIG_HALT {
                    (SIG_OK, value)
                } else {
                    (signal, value)
                }
            }
            Err(e) => (
                SIG_ERROR,
                crate::value::error_val("wasm-error", format!("WASM execution failed: {}", e)),
            ),
        };

        // Remove the temporary tier so the regular dispatch path doesn't use it.
        if !had_tier {
            self.wasm_tier = None;
        }
        result
    }

    /// Run a closure via the MLIR/LLVM CPU tier-2 backend.
    ///
    /// Requires `--features mlir`. The closure must satisfy the
    /// `is_mlir_cpu_eligible` predicate (no captures, exact arity, only
    /// arithmetic/comparison/local instructions). Arguments may be
    /// integers or floats — floats are bitcast f64→i64 by the caller
    /// and i64→f64 at MLIR function entry.
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

        let num_captures = closure.template.num_captures as u16;

        // Unbox captures to i64. They must be numeric (int or float).
        let mut int_args: Vec<i64> = Vec::with_capacity(closure.env.len() + args.len());
        let mut capture_types: u64 = 0;
        for i in 0..num_captures as usize {
            let v = closure.env[i];
            if let Some(n) = v.as_int() {
                int_args.push(n);
            } else if let Some(f) = v.as_float() {
                int_args.push(f.to_bits() as i64);
                capture_types |= 1u64 << i;
            } else {
                return (
                    SIG_ERROR,
                    rejected(
                        "mlir-cpu",
                        format!(
                            "capture {} is {}, not numeric; MLIR-CPU requires int/float captures",
                            i,
                            v.type_name()
                        ),
                    ),
                );
            }
        }

        // Unbox args to i64. Ints pass through; floats are bitcast f64→i64.
        let mut param_types: u64 = 0;
        for (i, v) in args.iter().enumerate() {
            if let Some(n) = v.as_int() {
                int_args.push(n);
            } else if let Some(f) = v.as_float() {
                int_args.push(f.to_bits() as i64);
                param_types |= 1u64 << i;
            } else {
                return (
                    SIG_ERROR,
                    rejected(
                        "mlir-cpu",
                        format!(
                            "arg {} is {}, not numeric; MLIR-CPU requires int/float args",
                            i,
                            v.type_name()
                        ),
                    ),
                );
            }
        }

        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let cache = self
            .mlir_cache
            .get_or_insert_with(crate::mlir::MlirCache::new);

        // Ensure compiled for this (capture_types, param_types) signature.
        if !cache.contains(bytecode_ptr, capture_types, param_types) {
            if let Err(e) =
                cache.compile(bytecode_ptr, &lir, num_captures, capture_types, param_types)
            {
                return (
                    SIG_ERROR,
                    rejected("mlir-cpu", format!("MLIR compilation failed: {}", e)),
                );
            }
        }

        // Reborrow as immutable for call.
        let cache = self.mlir_cache.as_ref().unwrap();
        match cache.call(bytecode_ptr, &int_args, capture_types, param_types) {
            Some(Ok(result)) => {
                // Rebox based on the compiled function's return type.
                let val = match cache.return_type(bytecode_ptr, capture_types, param_types) {
                    Some(crate::mlir::ScalarType::Float) => {
                        Value::float(f64::from_bits(result as u64))
                    }
                    Some(crate::mlir::ScalarType::Bool) => Value::bool(result != 0),
                    _ => Value::int(result),
                };
                (SIG_OK, val)
            }
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
