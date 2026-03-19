//! JIT compilation entry points and interpreter trampolines.
//!
//! Handles:
//! - JIT compilation profiling and caching
//! - JIT code execution and result dispatch
//! - Batch JIT compilation for call peers
//! - Fallback to interpreter on compilation failure

use crate::jit::{
    JitCode, JitCompiler, JitRejectionInfo, JitValue, TAIL_CALL_SENTINEL, YIELD_SENTINEL,
};
use crate::value::{SignalBits, SymbolId, Value, SIG_ERROR, SIG_HALT, SIG_YIELD};
use std::rc::Rc;

use super::core::VM;

impl VM {
    /// Try JIT compilation/dispatch for a closure call.
    ///
    /// Returns `Some(Option<SignalBits>)` if JIT handled the call (the inner
    /// Option follows handle_call's convention), or `None` to fall through
    /// to the interpreter path. Caller is responsible for decrementing
    /// call_depth on the `Some` path.
    pub(super) fn try_jit_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<Option<SignalBits>> {
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let is_hot = self.record_closure_call(bytecode_ptr);

        // Check if we already have JIT code for this closure
        if let Some(jit_code) = self.jit_cache.get(&bytecode_ptr).cloned() {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        // If hot, attempt JIT compilation
        if is_hot {
            if let Some(ref lir_func) = closure.template.lir_function {
                // Hoist the SymbolId lookup — needed for both batch and solo paths
                let self_sym = self.find_global_sym_for_bytecode(bytecode_ptr);

                // Try batch compilation first for capture-free functions
                if lir_func.num_captures == 0 {
                    if let Some(result) =
                        self.try_batch_jit(lir_func, bytecode_ptr, closure, args, func, self_sym)
                    {
                        return Some(result);
                    }
                }

                // Solo compilation — pass self_sym for direct self-calls
                match JitCompiler::new() {
                    Ok(compiler) => match compiler.compile(lir_func, self_sym) {
                        Ok(jit_code) => {
                            let jit_code = Rc::new(jit_code);
                            self.jit_cache.insert(bytecode_ptr, jit_code.clone());
                            return Some(self.run_jit(&jit_code, closure, args, func));
                        }
                        Err(e) => match &e {
                            crate::jit::JitError::UnsupportedInstruction(_)
                            | crate::jit::JitError::Polymorphic => {
                                // Expected rejection — record and fall back to interpreter.
                                self.record_jit_rejection(bytecode_ptr, closure, e);
                            }
                            _ => {
                                panic!(
                                    "JIT compilation failed for function: {}. Error: {}",
                                    closure
                                        .template
                                        .lir_function
                                        .as_ref()
                                        .map(|f| f.name.as_deref().unwrap_or("<anon>"))
                                        .unwrap_or("<no lir>"),
                                    e
                                );
                            }
                        },
                    },
                    Err(e) => {
                        panic!("JIT compiler creation failed: {}. This is a bug.", e);
                    }
                }
            }
        }

        None // Fall through to interpreter
    }

    /// Run JIT-compiled code and handle the result.
    ///
    /// Returns `Option<SignalBits>` following handle_call's convention:
    /// `None` to continue dispatch, `Some(bits)` to return immediately.
    fn run_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<SignalBits> {
        let result = self.call_jit(jit_code, closure, args, func);

        // Check if the JIT function (or a callee) set an error or halt
        if self
            .fiber
            .signal
            .as_ref()
            .is_some_and(|(b, _)| b.contains(SIG_ERROR) || b.contains(SIG_HALT))
        {
            self.fiber.stack.push(Value::NIL);
            return None;
        }

        // Check for yield sentinel (JIT function yielded directly)
        if result == YIELD_SENTINEL {
            let sig = self
                .fiber
                .signal
                .as_ref()
                .map(|(b, _)| *b)
                .unwrap_or(SIG_YIELD);
            return Some(sig);
        }

        // Check for pending tail call (JIT function did a TailCall)
        if result == TAIL_CALL_SENTINEL {
            if let Some(tail) = self.pending_tail_call.take() {
                let exec_result = self.execute_bytecode_saving_stack(
                    &tail.bytecode,
                    &tail.constants,
                    &tail.env,
                    &tail.location_map,
                );
                let eb = exec_result.bits;
                if eb.is_ok() || eb == SIG_HALT {
                    let (_, val) = self.fiber.signal.take().unwrap();
                    self.fiber.stack.push(val);
                    return None;
                } else if eb.contains(SIG_YIELD) {
                    return Some(SIG_YIELD);
                } else {
                    // SIG_ERROR — signal already set on fiber
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            }
        }

        // Normal result: reconstruct Value from JitValue
        self.fiber.stack.push(result.to_value());
        None
    }

    /// Call a JIT-compiled function.
    ///
    /// # Safety
    /// The JIT code must have been compiled from the same LIR function that
    /// produced the closure's bytecode. The calling convention must match.
    ///
    /// `func_value` is the original Value representing the closure, used for
    /// self-tail-call detection in the JIT code.
    fn call_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
        func_value: Value,
    ) -> JitValue {
        let env_ptr = if closure.env.is_empty() {
            std::ptr::null()
        } else {
            closure.env.as_ptr()
        };

        unsafe {
            jit_code.call(
                env_ptr,
                args.as_ptr(),
                args.len() as u32,
                self as *mut VM as *mut (),
                func_value.tag,
                func_value.payload,
            )
        }
    }

    /// Try batch JIT compilation for a hot function and its call peers.
    ///
    /// Returns `Some` if batch compilation succeeded and the hot function
    /// was executed. Returns `None` to fall through to solo compilation.
    fn try_batch_jit(
        &mut self,
        lir_func: &Rc<crate::lir::LirFunction>,
        _bytecode_ptr: *const u8,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
        hot_sym: Option<SymbolId>,
    ) -> Option<Option<SignalBits>> {
        // With globals removed, discover_compilation_group always returns
        // empty — batch JIT requires compile-time peer discovery (future work).
        let group = crate::jit::discover_compilation_group(lir_func, &[]);
        if group.is_empty() {
            return None;
        }

        let hot_sym = hot_sym?;

        let compiler = match JitCompiler::new() {
            Ok(c) => c,
            Err(e) => {
                panic!("JIT compiler creation failed: {}. This is a bug.", e);
            }
        };

        let mut members = Vec::with_capacity(group.len() + 1);
        members.push(crate::jit::BatchMember {
            sym: hot_sym,
            lir: lir_func,
        });

        for (sym, lir) in &group {
            if *sym != hot_sym {
                members.push(crate::jit::BatchMember { sym: *sym, lir });
            }
        }

        if members.len() <= 1 {
            return None;
        }

        let results = match compiler.compile_batch(&members) {
            Ok(r) => r,
            Err(e) => match &e {
                crate::jit::JitError::UnsupportedInstruction(_)
                | crate::jit::JitError::Yielding => {
                    return None;
                }
                _ => {
                    panic!(
                        "Batch JIT compilation failed: {}. \
                         This is a bug — all members were pre-validated as JIT-compilable.",
                        e
                    );
                }
            },
        };

        // Insert all compiled functions into cache and find the hot one
        let mut hot_jit_code = None;
        for (sym, jit_code) in results {
            let jit_code = Rc::new(jit_code);
            if sym == hot_sym {
                let bc_ptr = closure.template.bytecode.as_ptr();
                self.jit_cache.insert(bc_ptr, jit_code.clone());
                hot_jit_code = Some(jit_code);
            }
        }

        if let Some(jit_code) = hot_jit_code {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        None
    }

    /// Record a JIT rejection for a closure. Only the first rejection per
    /// closure template is stored (deduplicated by bytecode pointer).
    fn record_jit_rejection(
        &mut self,
        bytecode_ptr: *const u8,
        closure: &crate::value::Closure,
        error: crate::jit::JitError,
    ) {
        self.jit_rejections.entry(bytecode_ptr).or_insert_with(|| {
            // Prefer LIR name, fall back to closure template name.
            let name = closure
                .template
                .lir_function
                .as_ref()
                .and_then(|f| f.name.clone())
                .or_else(|| closure.template.name.as_ref().map(|n| n.to_string()));
            JitRejectionInfo {
                name,
                reason: error,
            }
        });
    }

    /// Find the SymbolId for a closure matching the given bytecode pointer.
    ///
    /// With globals removed, there is no global symbol table to scan.
    /// Always returns `None`. Solo JIT compilation still works — it just
    /// won't emit direct self-calls (falls back to `elle_jit_call`).
    fn find_global_sym_for_bytecode(&self, _bytecode_ptr: *const u8) -> Option<SymbolId> {
        None
    }
}
