//! `compile/run-on :mlir-cpu` — force MLIR/LLVM CPU tier-2 execution
//! (`--features mlir`).

use super::rejected;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};
use crate::vm::core::VM;

impl VM {
    /// Run a closure via the MLIR/LLVM CPU tier-2 backend.
    ///
    /// Requires `--features mlir`. The closure must satisfy the
    /// `is_mlir_cpu_eligible` predicate (no captures, exact arity, only
    /// arithmetic/comparison/local instructions). Arguments may be
    /// integers or floats — floats are bitcast f64→i64 by the caller
    /// and i64→f64 at MLIR function entry.
    pub fn invoke_closure_mlir_cpu(
        &mut self,
        _closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        let lir = match closure.template.lir_function() {
            Some(l) => std::rc::Rc::clone(l),
            None => return (SIG_ERROR, rejected(self, "mlir-cpu", "closure has no LIR")),
        };

        if !lir.is_mlir_cpu_eligible() {
            return (
                SIG_ERROR,
                rejected(self, "mlir-cpu", "closure is not MLIR-CPU eligible"),
            );
        }

        if !self.check_arity(&closure.template.arity(), args.len()) {
            return self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
        }

        let num_captures = closure.template.num_captures() as u16;

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
                        self,
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
                        self,
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

        let bytecode_ptr = closure.template.bytecode().as_ptr();
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
                    rejected(self, "mlir-cpu", format!("MLIR compilation failed: {}", e)),
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
                self.escaping_error("mlir-error", format!("MLIR execution failed: {}", e)),
            ),
            None => (
                SIG_ERROR,
                rejected(self, "mlir-cpu", "MLIR cache miss after compile (bug)"),
            ),
        }
    }
}
