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
        let num_captures = closure.template.num_captures as u16;

        // Build capture_types bitmask from the closure's environment.
        let mut capture_types: u64 = 0;
        for i in 0..num_captures as usize {
            let v = closure.env[i];
            if v.as_float().is_some() {
                capture_types |= 1u64 << i;
            } else if v.as_int().is_none() {
                return None; // non-numeric capture — fall through
            }
        }

        // Build param_types bitmask: bit i = 1 means param i is Float.
        let mut param_types: u64 = 0;
        for (i, v) in args.iter().enumerate() {
            if v.as_float().is_some() {
                param_types |= 1u64 << i;
            } else if v.as_int().is_none() {
                return None; // non-numeric arg — fall through
            }
        }

        let cache = self
            .mlir_cache
            .get_or_insert_with(crate::mlir::MlirCache::new);
        if cache.contains(bytecode_ptr, capture_types, param_types) {
            return self.run_mlir_cached(closure, bytecode_ptr, args, capture_types, param_types);
        }
        if cache.is_rejected(bytecode_ptr, capture_types, param_types) {
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
        // must round-trip through i64 correctly.
        let lir = closure.template.lir_function.as_ref()?;
        if !lir.is_mlir_cpu_eligible() {
            return None;
        }

        // Compile via MLIR
        let cache = self.mlir_cache.as_mut().unwrap();
        match cache.compile(bytecode_ptr, lir, num_captures, capture_types, param_types) {
            Ok(_name) => {
                if crate::config::get().debug_jit {
                    eprintln!(
                        "[mlir] compiled: {}",
                        closure.template.name.as_deref().unwrap_or("<anon>")
                    );
                }
                self.run_mlir_cached(closure, bytecode_ptr, args, capture_types, param_types)
            }
            Err(e) => {
                // Cache rejection so we don't retry on every call
                self.mlir_cache
                    .as_mut()
                    .unwrap()
                    .reject(bytecode_ptr, capture_types, param_types);
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

    /// Execute a cached MLIR function, unboxing captures+args and reboxing the result.
    ///
    /// Returns:
    /// - `None` — MLIR can't handle this call (non-numeric arg or cache miss);
    ///   caller should fall through to Cranelift/interpreter.
    /// - `Some(None)` — handled, no signal.
    /// - `Some(Some(bits))` — handled with signal (error stored in fiber.signal).
    fn run_mlir_cached(
        &mut self,
        closure: &crate::value::Closure,
        bytecode_ptr: *const u8,
        args: &[Value],
        capture_types: u64,
        param_types: u64,
    ) -> Option<Option<SignalBits>> {
        let num_captures = closure.template.num_captures;

        // Unbox captures + args: ints pass through, floats bitcast f64→i64.
        let mut i64_args: Vec<i64> = Vec::with_capacity(num_captures + args.len());

        // Captures first
        for i in 0..num_captures {
            let v = closure.env[i];
            if capture_types & (1u64 << i) != 0 {
                match v.as_float() {
                    Some(f) => i64_args.push(f.to_bits() as i64),
                    None => return None,
                }
            } else {
                match v.as_int() {
                    Some(n) => i64_args.push(n),
                    None => return None,
                }
            }
        }

        // Then params
        for (i, v) in args.iter().enumerate() {
            if param_types & (1u64 << i) != 0 {
                match v.as_float() {
                    Some(f) => i64_args.push(f.to_bits() as i64),
                    None => return None,
                }
            } else {
                match v.as_int() {
                    Some(n) => i64_args.push(n),
                    None => return None,
                }
            }
        }

        let cache = self.mlir_cache.as_ref().unwrap();
        match cache.call(bytecode_ptr, &i64_args, capture_types, param_types) {
            Some(Ok(result)) => {
                // Rebox based on the compiled function's return type.
                let val = match cache.return_type(bytecode_ptr, capture_types, param_types) {
                    Some(crate::mlir::ScalarType::Float) => {
                        Value::float(f64::from_bits(result as u64))
                    }
                    Some(crate::mlir::ScalarType::Bool) => Value::bool(result != 0),
                    _ => Value::int(result),
                };
                self.fiber.stack.push(val);
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
