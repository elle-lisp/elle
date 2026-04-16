//! MLIR tier-2 compilation entry point.
//!
//! GPU-eligible closures that are already hot (past the JIT threshold)
//! are compiled through MLIR → LLVM for optimized native execution.
//! The MLIR cache is lazily initialized on first use.

use crate::value::{SignalBits, Value, SIG_ERROR};

use super::core::VM;

impl VM {
    /// Try MLIR compilation/dispatch for a GPU-eligible closure.
    ///
    /// Returns `Some(None)` if MLIR handled the call (result on stack),
    /// or `None` to fall through to the Cranelift/interpreter path.
    pub(super) fn try_mlir_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Option<SignalBits>> {
        // Only GPU-eligible closures qualify for MLIR
        if !closure.template.is_gpu_candidate() {
            return None;
        }

        let bytecode_ptr = closure.template.bytecode.as_ptr();

        // Check cache first (fast path)
        let cache = self
            .mlir_cache
            .get_or_insert_with(crate::mlir::MlirCache::new);
        if cache.contains(bytecode_ptr) {
            return self.run_mlir_cached(bytecode_ptr, args);
        }
        if cache.is_rejected(bytecode_ptr) {
            return None;
        }

        // Check hotness without incrementing — the counter is owned
        // by try_jit_call which runs after us. We just read it.
        let count = self.get_closure_call_count(bytecode_ptr);
        if count < self.jit_hotness_threshold {
            return None;
        }

        // Full LIR instruction walk (only for hot functions).
        // Use the stricter MLIR-CPU eligibility check: the return register
        // must round-trip through i64 as an integer, not as nil/bool/compare.
        let lir = closure.template.lir_function.as_ref()?;
        if !lir.is_mlir_cpu_eligible() {
            return None;
        }

        // Compile via MLIR
        let cache = self.mlir_cache.as_mut().unwrap();
        match cache.compile(bytecode_ptr, lir) {
            Ok(_name) => {
                if crate::config::get().debug_jit {
                    eprintln!(
                        "[mlir] compiled: {}",
                        closure.template.name.as_deref().unwrap_or("<anon>")
                    );
                }
                self.run_mlir_cached(bytecode_ptr, args)
            }
            Err(e) => {
                // Cache rejection so we don't retry on every call
                self.mlir_cache.as_mut().unwrap().reject(bytecode_ptr);
                if crate::config::get().debug_jit {
                    eprintln!(
                        "[mlir] failed {}: {}",
                        closure.template.name.as_deref().unwrap_or("<anon>"),
                        e
                    );
                }
                None // fall through to Cranelift
            }
        }
    }

    /// Execute a cached MLIR function, unboxing args and reboxing the result.
    ///
    /// Returns:
    /// - `None` — MLIR can't handle this call (non-int arg or cache miss);
    ///   caller should fall through to Cranelift/interpreter.
    /// - `Some(None)` — handled, no signal.
    /// - `Some(Some(bits))` — handled with signal (error stored in fiber.signal).
    fn run_mlir_cached(
        &mut self,
        bytecode_ptr: *const u8,
        args: &[Value],
    ) -> Option<Option<SignalBits>> {
        // Type check: all args must be integers. Other types (nil, bool, etc.)
        // can't round-trip through i64 without losing type information —
        // the bytecode path handles those correctly.
        let mut i64_args: Vec<i64> = Vec::with_capacity(args.len());
        for v in args {
            match v.as_int() {
                Some(n) => i64_args.push(n),
                None => return None, // non-int arg — fall through to bytecode
            }
        }

        let cache = self.mlir_cache.as_ref().unwrap();
        match cache.call(bytecode_ptr, &i64_args) {
            Some(Ok(result)) => {
                self.fiber.stack.push(Value::int(result));
                Some(None) // handled, no signal
            }
            Some(Err(_)) => {
                let err =
                    crate::value::error_val("mlir-error", "MLIR execution failed".to_string());
                self.fiber.signal = Some((SIG_ERROR, err));
                self.fiber.stack.push(Value::NIL);
                Some(Some(SIG_ERROR))
            }
            None => None,
        }
    }
}
