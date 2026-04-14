//! MLIR tier-2 compilation entry point.
//!
//! GPU-eligible closures that are already hot (past the JIT threshold)
//! are compiled through MLIR → LLVM for optimized native execution.
//! The MLIR cache is lazily initialized on first use.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};

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
        if let Some(cache) = &self.mlir_cache {
            if cache.contains(bytecode_ptr) {
                return Some(self.run_mlir_cached(bytecode_ptr, args));
            }
        }

        // Check hotness without incrementing — the counter is owned
        // by try_jit_call which runs after us. We just read it.
        let count = self.get_closure_call_count(bytecode_ptr);
        if count < self.jit_hotness_threshold {
            return None;
        }

        // Full LIR instruction walk (only for hot functions)
        let lir = closure.template.lir_function.as_ref()?;
        if !lir.is_gpu_eligible() {
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
                Some(self.run_mlir_cached(bytecode_ptr, args))
            }
            Err(e) => {
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
    fn run_mlir_cached(&mut self, bytecode_ptr: *const u8, args: &[Value]) -> Option<SignalBits> {
        // Unbox: extract i64 payload from each Value
        let i64_args: Vec<i64> = args.iter().map(|v| v.as_int().unwrap_or(0)).collect();

        let cache = self.mlir_cache.as_ref().unwrap();
        match cache.call(bytecode_ptr, &i64_args) {
            Some(Ok(result)) => {
                self.fiber.stack.push(Value::int(result));
                None // no signal
            }
            Some(Err(_)) => {
                let err =
                    crate::value::error_val("mlir-error", "MLIR execution failed".to_string());
                self.fiber.signal = Some((SIG_ERROR, err));
                self.fiber.stack.push(Value::NIL);
                None
            }
            None => None,
        }
    }
}
