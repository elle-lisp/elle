//! JIT compilation entry points and interpreter trampolines.
//!
//! Handles:
//! - JIT compilation profiling and caching
//! - JIT code execution and result dispatch
//! - Batch JIT compilation for call peers
//! - Fallback to interpreter on compilation failure

use crate::jit::{JitCode, JitRejectionInfo, JitValue, TAIL_CALL_SENTINEL, YIELD_SENTINEL};
use crate::value::{SignalBits, SymbolId, Value, SIG_ERROR, SIG_HALT, SIG_YIELD};
use std::sync::Arc;

use super::core::VM;

impl VM {
    /// Try JIT compilation/dispatch for a closure call.
    ///
    /// Returns `Some(Option<SignalBits>)` if JIT handled the call (the inner
    /// Option follows handle_call's convention), or `None` to fall through
    /// to the interpreter path. Caller is responsible for decrementing
    /// call_depth on the `Some` path.
    ///
    /// Compilation is asynchronous: when a function becomes hot, its LIR
    /// is sent to a background thread for Cranelift compilation. The
    /// interpreter continues running the function until compiled code
    /// is ready. Zero stall on the event loop.
    pub(super) fn try_jit_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<Option<SignalBits>> {
        if !self.jit_enabled {
            return None;
        }
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let is_hot = self.record_closure_call(bytecode_ptr);

        // Poll for completed background compilations (cheap: non-blocking recv)
        self.poll_jit_completions();

        // Check cache (may have been populated by poll above)
        if let Some(jit_code) = self.jit_cache.get(&bytecode_ptr).cloned() {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        // If hot and not already pending, submit background compilation
        if is_hot && !self.jit_pending.contains(&(bytecode_ptr as usize)) {
            if let Some(ref lir_func) = closure.template.lir_function {
                self.submit_jit_task(lir_func, closure, bytecode_ptr);
            }
        }

        None // Interpreter fallback while compilation proceeds in background
    }

    /// Poll the background JIT worker for completed compilations.
    /// Inserts successful results into jit_cache; records rejections.
    fn poll_jit_completions(&mut self) {
        let worker = match self.jit_worker.as_ref() {
            Some(w) => w,
            None => return,
        };
        let results: Vec<_> = worker.poll().collect();
        for result in results {
            self.jit_pending.remove(&result.bytecode_key);
            match result.result {
                Ok(jit_code) => {
                    let bytecode_ptr = result.bytecode_key as *const u8;
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::JIT)
                    {
                        eprintln!(
                            "[jit] background compiled: bc_ptr={:#x}",
                            result.bytecode_key,
                        );
                    }
                    self.jit_cache.insert(bytecode_ptr, Arc::new(jit_code));
                }
                Err(e) => match &e {
                    crate::jit::JitError::UnsupportedInstruction(_)
                    | crate::jit::JitError::Polymorphic
                    | crate::jit::JitError::Yielding => {
                        // Expected rejection — record for diagnostics.
                        let bytecode_ptr = result.bytecode_key as *const u8;
                        self.jit_rejections.entry(bytecode_ptr).or_insert_with(|| {
                            JitRejectionInfo {
                                name: None,
                                reason: e,
                            }
                        });
                    }
                    _ => {
                        eprintln!("[jit] background compilation failed: {}", e);
                    }
                },
            }
        }
    }

    /// Submit a background JIT compilation task for a hot function.
    fn submit_jit_task(
        &mut self,
        lir_func: &crate::lir::LirFunction,
        closure: &crate::value::Closure,
        bytecode_ptr: *const u8,
    ) {
        let self_sym = self.find_global_sym_for_bytecode(bytecode_ptr);
        let task = crate::jit::worker::prepare_task(
            lir_func,
            self_sym,
            (*closure.template.symbol_names).clone(),
            bytecode_ptr as usize,
        );

        // Lazily spawn the worker thread on first use
        let worker = self
            .jit_worker
            .get_or_insert_with(crate::jit::worker::JitWorker::new);

        if worker.submit(task) {
            self.jit_pending.insert(bytecode_ptr as usize);
            if self
                .runtime_config
                .has_trace_bit(crate::config::trace_bits::JIT)
            {
                eprintln!(
                    "[jit] submitted background compilation: name={} bc_ptr={:#x}",
                    closure.template.name.as_deref().unwrap_or("<anon>"),
                    bytecode_ptr as usize,
                );
            }
        }
    }

    /// Block until all pending background JIT compilations complete.
    /// Used by `jit/rejections` and `--stats` to ensure all results
    /// are available before reporting.
    pub fn drain_jit_pending(&mut self) {
        while !self.jit_pending.is_empty() {
            let worker = match self.jit_worker.as_ref() {
                Some(w) => w,
                None => break,
            };
            match worker.recv_blocking() {
                Some(result) => {
                    self.jit_pending.remove(&result.bytecode_key);
                    match result.result {
                        Ok(jit_code) => {
                            let bytecode_ptr = result.bytecode_key as *const u8;
                            self.jit_cache.insert(bytecode_ptr, Arc::new(jit_code));
                        }
                        Err(e) => {
                            let bytecode_ptr = result.bytecode_key as *const u8;
                            self.jit_rejections.entry(bytecode_ptr).or_insert_with(|| {
                                JitRejectionInfo {
                                    name: None,
                                    reason: e,
                                }
                            });
                        }
                    }
                }
                None => break, // Worker exited
            }
        }
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

            // Squelch enforcement: if the closure has a squelch mask and the
            // signal matches, convert to signal-violation error.
            let squelch_mask = closure.squelch_mask;
            if !squelch_mask.is_empty() && !sig.contains(SIG_ERROR) && !sig.contains(SIG_HALT) {
                let squelched = sig.intersection(squelch_mask);
                if !squelched.is_empty() {
                    let squelched_str = {
                        let registry = crate::signals::registry::global_registry().lock().unwrap();
                        registry.format_signal_bits(squelched)
                    };
                    let err = crate::value::error_val(
                        "signal-violation",
                        format!("squelch: signal {} caught at boundary", squelched_str),
                    );
                    self.fiber.suspended = None;
                    self.fiber.signal = Some((SIG_ERROR, err));
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            }

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
                if eb.is_ok() {
                    let (_, val) = self.fiber.signal.take().unwrap();
                    self.fiber.stack.push(val);
                    return None;
                } else if eb == SIG_HALT {
                    // (halt) → NIL → absorb. (halt <value>) → let dispatch loop catch it.
                    let val = self
                        .fiber
                        .signal
                        .as_ref()
                        .map(|(_, v)| *v)
                        .unwrap_or(Value::NIL);
                    if val == Value::NIL {
                        self.fiber.signal.take();
                        self.fiber.stack.push(Value::NIL);
                        return None;
                    }
                    // Non-NIL halt: leave signal in place, dispatch loop will see it.
                    self.fiber.stack.push(Value::NIL);
                    return None;
                } else if eb.contains(SIG_ERROR) {
                    // SIG_ERROR — signal already set on fiber
                    self.fiber.stack.push(Value::NIL);
                    return None;
                } else {
                    // Suspending signal (SIG_YIELD, SIG_SWITCH, user-defined).
                    // Squelch enforcement on the tail-call path
                    let tail_squelch = tail.squelch_mask | closure.squelch_mask;
                    if !tail_squelch.is_empty() {
                        let squelched = eb.intersection(tail_squelch);
                        if !squelched.is_empty() {
                            let squelched_str = {
                                let registry =
                                    crate::signals::registry::global_registry().lock().unwrap();
                                registry.format_signal_bits(squelched)
                            };
                            let err = crate::value::error_val(
                                "signal-violation",
                                format!("squelch: signal {} caught at boundary", squelched_str),
                            );
                            self.fiber.suspended = None;
                            self.fiber.signal = Some((SIG_ERROR, err));
                            self.fiber.stack.push(Value::NIL);
                            return None;
                        }
                    }
                    // Propagate so call_inner can build the caller frame.
                    return Some(eb);
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
    pub(crate) fn call_jit(
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

        // Save/restore rotation base so nested self-tail-call loops
        // don't corrupt the caller's rotation state.
        let saved_rotation_base =
            crate::value::fiberheap::with_current_heap_mut(|h| h.save_jit_rotation_base())
                .flatten();

        let result = unsafe {
            jit_code.call(
                env_ptr,
                args.as_ptr(),
                args.len() as u32,
                self as *mut VM as *mut (),
                func_value.tag,
                func_value.payload,
            )
        };

        crate::value::fiberheap::with_current_heap_mut(|h| {
            h.restore_jit_rotation_base(saved_rotation_base.clone());
        });

        result
    }

    /// Try batch JIT compilation for a hot function and its call peers.
    ///
    /// Find the SymbolId for a closure matching the given bytecode pointer.
    ///
    /// With globals removed, there is no global symbol table to scan.
    /// Always returns `None`. Solo JIT compilation still works — it just
    /// won't emit direct self-calls (falls back to `elle_jit_call`).
    fn find_global_sym_for_bytecode(&self, _bytecode_ptr: *const u8) -> Option<SymbolId> {
        None
    }
}
