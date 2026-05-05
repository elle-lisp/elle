//! WASM tiered compilation entry points.
//!
//! When `--wasm=N`, hot closures are compiled to per-closure WASM
//! modules and dispatched through Wasmtime. This mirrors the JIT path
//! in `jit_entry.rs` but targets WASM instead of Cranelift.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT};

use super::core::VM;

impl VM {
    /// Try WASM compilation/dispatch for a closure call.
    ///
    /// Returns `Some(Option<SignalBits>)` if WASM handled the call (inner
    /// Option follows handle_call's convention), or `None` to fall through.
    pub(super) fn try_wasm_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Option<SignalBits>> {
        let wasm_tier = self.wasm_tier.as_ref()?;
        let bytecode_ptr = closure.template.bytecode.as_ptr();

        // Skip if already rejected
        if self.wasm_rejections.contains_key(&bytecode_ptr) {
            return None;
        }

        // Check if already compiled
        if wasm_tier.is_compiled(bytecode_ptr) {
            return Some(self.run_wasm(bytecode_ptr, closure, args));
        }

        // Check if hot enough to compile
        // (call count already incremented by try_jit_call or record_closure_call)
        let count = self
            .closure_call_counts
            .get(&bytecode_ptr)
            .copied()
            .unwrap_or(0);
        if count < self.jit_hotness_threshold {
            return None;
        }

        // Need LIR to compile
        let lir_func = match &closure.template.lir_function {
            Some(f) => f.clone(),
            None => return None,
        };

        // Try to compile
        let wasm_tier = self.wasm_tier.as_mut().unwrap();
        if wasm_tier.compile(bytecode_ptr, &lir_func) {
            return Some(self.run_wasm(bytecode_ptr, closure, args));
        }

        // Compilation rejected — record so we don't try again
        self.wasm_rejections.insert(bytecode_ptr, ());
        None
    }

    /// Run a WASM-compiled closure and handle the result.
    fn run_wasm(
        &mut self,
        bytecode_ptr: *const u8,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<SignalBits> {
        let closure_rc = std::rc::Rc::new(closure.clone());
        let vm_ptr = self as *mut VM;

        let wasm_tier = self.wasm_tier.as_ref().unwrap();
        match wasm_tier.call(vm_ptr, bytecode_ptr, &closure_rc, args) {
            Ok((value, signal)) => {
                if signal.is_ok() {
                    self.fiber.stack.push(value);
                    None
                } else if signal == SIG_HALT {
                    if value == Value::NIL {
                        self.fiber.stack.push(value);
                        return None;
                    }
                    // Non-NIL halt: set signal, dispatch loop will catch it
                    self.fiber.signal = Some((signal, value));
                    self.fiber.stack.push(Value::NIL);
                    None
                } else if signal.contains(SIG_ERROR) {
                    // Error — set signal on fiber
                    self.fiber.signal = Some((signal, value));
                    self.fiber.stack.push(Value::NIL);
                    None
                } else {
                    // Other signal (shouldn't happen — we reject yielding closures)
                    Some(signal)
                }
            }
            Err(e) => {
                // WASM execution error — convert to Elle error
                let err = crate::value::error_val("internal-error", format!("wasm: {}", e));
                self.fiber.signal = Some((SIG_ERROR, err));
                self.fiber.stack.push(Value::NIL);
                None
            }
        }
    }
}
