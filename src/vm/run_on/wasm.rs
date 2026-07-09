//! `compile/run-on :wasm` — force Wasmtime tiered execution (`--features wasm`).

use super::rejected;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};
use crate::vm::core::VM;
use std::rc::Rc;

impl VM {
    /// Run a closure via the WASM backend (Wasmtime tiered compilation).
    ///
    /// Requires `--features wasm`. Force-compiles the closure to a
    /// per-closure WASM module and dispatches through Wasmtime.
    pub fn invoke_closure_wasm(
        &mut self,
        closure_val: Value,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> (SignalBits, Value) {
        let lir = match closure.template.lir_function.clone() {
            Some(l) => l,
            None => return (SIG_ERROR, rejected(self, "wasm", "closure has no LIR")),
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
                    self,
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
                        rejected(self, "wasm", format!("WasmTier init failed: {}", e)),
                    )
                }
            }
        }

        // Force-compile if not already cached.
        let heap_ptr = self.heap_ptr;
        let tier = self.wasm_tier.as_mut().unwrap();
        if !tier.is_compiled(bytecode_ptr) && !tier.compile(bytecode_ptr, &lir, heap_ptr) {
            // Remove the temporary tier before returning.
            if !had_tier {
                self.wasm_tier = None;
            }
            return (
                SIG_ERROR,
                rejected(self, "wasm", "WASM compilation rejected this closure"),
            );
        }

        // Call through Wasmtime. The result comes back as (Value, SignalBits).
        let closure_rc = Rc::new(closure.clone());
        let vm_ptr = self as *mut VM;
        let tier = self.wasm_tier.as_ref().unwrap();

        let result = match tier.call(vm_ptr, bytecode_ptr, &closure_rc, args, closure_val) {
            Ok((value, signal)) => {
                if signal.is_ok() {
                    (SIG_OK, value)
                } else if signal == crate::value::SIG_HALT {
                    if value == Value::NIL {
                        (SIG_OK, value)
                    } else {
                        (signal, value)
                    }
                } else {
                    (signal, value)
                }
            }
            Err(e) => (
                SIG_ERROR,
                self.escaping_error("wasm-error", format!("WASM execution failed: {}", e)),
            ),
        };

        // Remove the temporary tier so the regular dispatch path doesn't use it.
        if !had_tier {
            self.wasm_tier = None;
        }
        result
    }
}
