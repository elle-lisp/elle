//! JIT compilation entry points and interpreter trampolines.
//!
//! Handles:
//! - JIT compilation profiling and caching
//! - JIT code execution and result dispatch
//! - Batch JIT compilation for call peers
//! - Fallback to interpreter on compilation failure

use crate::jit::{JitCode, JitRejectionInfo, JitValue, TAIL_CALL_SENTINEL, YIELD_SENTINEL};
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_YIELD};
use std::sync::Arc;

use super::core::VM;

#[cfg(test)]
mod tests;

impl VM {
    /// Install compiled code for the function whose bytecode is `bytecode`.
    /// The single write path into `jit_cache`: the key is derived from the
    /// bytecode here, never passed separately, and the entry pins the
    /// bytecode so the key stays sound (docs/impl/jit.md § "Cache identity").
    pub fn install_jit_code(&mut self, bytecode: std::rc::Rc<Vec<u8>>, code: Arc<JitCode>) {
        self.jit_cache.insert(
            bytecode.as_ptr(),
            crate::vm::core::JitCacheEntry::new(bytecode, code),
        );
    }

    /// Compiled code for the function at `bytecode_ptr`, if cached.
    pub fn jit_code_for(&self, bytecode_ptr: *const u8) -> Option<Arc<JitCode>> {
        self.jit_cache.get(&bytecode_ptr).map(|e| e.code.clone())
    }

    /// Record that a compile for `bytecode` is in flight on the worker. The
    /// entry pins `bytecode` until the result installs, so the key the
    /// worker echoes back still names this function.
    pub(crate) fn record_jit_pending(&mut self, bytecode: std::rc::Rc<Vec<u8>>) {
        self.jit_pending
            .insert(bytecode.as_ptr() as usize, bytecode);
    }

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
        if !self.runtime_config.jit.enabled() {
            return None;
        }
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        let is_hot = self.record_closure_call(bytecode_ptr);

        // Poll for completed background compilations (cheap: non-blocking recv)
        self.poll_jit_completions();

        // Check cache (may have been populated by poll above)
        if let Some(jit_code) = self.jit_code_for(bytecode_ptr) {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        // If hot, not already pending, and not already rejected, submit
        // background compilation. The rejection check is the negative cache
        // (docs/impl/jit.md "Rejection tracking"): a function whose LIR the
        // JIT has rejected can only ever reproduce the identical rejection, so
        // re-submitting is pure wasted work. Under eager JIT (threshold 0,
        // every call "hot") the absence of this check re-submits un-jit'able
        // hot functions on every call and saturates the background worker.
        if is_hot
            && !self.jit_pending.contains_key(&(bytecode_ptr as usize))
            && !self.jit_rejections.contains_key(&bytecode_ptr)
        {
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
            // The pin recorded at submit is what keeps `bytecode_key`
            // naming this function; without it the address may already
            // belong to a different function's bytecode, so the result
            // must not be installed under it.
            let pin = self.jit_pending.remove(&result.bytecode_key);
            match result.result {
                Ok(jit_code) => {
                    let Some(pin) = pin else { continue };
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::JIT)
                    {
                        eprintln!(
                            "[jit] background compiled: bc_ptr={:#x}",
                            result.bytecode_key,
                        );
                    }
                    self.install_jit_code(pin, Arc::new(jit_code));
                }
                Err(e) => match &e {
                    crate::jit::JitError::UnsupportedInstruction(_)
                    | crate::jit::JitError::Polymorphic
                    | crate::jit::JitError::Yielding => {
                        // Expected rejection — record for diagnostics.
                        let bytecode_ptr = result.bytecode_key as *const u8;
                        self.jit_rejections
                            .entry(bytecode_ptr)
                            .or_insert_with(|| JitRejectionInfo::new(e, pin));
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
        let label = closure.template.display_label();
        let bytecode = closure.template.bytecode.clone();
        let task =
            crate::jit::worker::prepare_task(lir_func, None, bytecode_ptr as usize, Some(&label));

        // `--trace=syncjit`: compile here on the VM thread and install
        // immediately; the `elle-jit` worker never spawns. Codegen inputs are
        // identical to the background path (same prepare_task output), so a
        // failure that persists under syncjit indicts codegen or its inputs,
        // while one that vanishes lives at the worker boundary — the Send
        // claim on JitTask, or a poll/install racing execution. Diagnosing a
        // suspected JIT race starts here; `--trace=jit,syncjit` logs each
        // synchronous install like the background path logs its own.
        if crate::config::get().has_trace("syncjit") {
            let res = crate::jit::JitCompiler::new()
                .and_then(|c| c.compile(&task.lir, task.self_sym, Vec::new()));
            match res {
                Ok(jit_code) => {
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::JIT)
                    {
                        eprintln!(
                            "[jit] synchronous compile (syncjit): bc_ptr={:#x}",
                            bytecode_ptr as usize,
                        );
                    }
                    self.install_jit_code(bytecode, Arc::new(jit_code));
                }
                Err(e) => {
                    self.jit_rejections
                        .entry(bytecode_ptr)
                        .or_insert_with(|| JitRejectionInfo::new(e, Some(bytecode)));
                }
            }
            *self.jit_compile_attempts.entry(bytecode_ptr).or_insert(0) += 1;
            return;
        }

        // Lazily spawn the worker thread on first use
        let worker = self
            .jit_worker
            .get_or_insert_with(crate::jit::worker::JitWorker::new);

        if worker.submit(task) {
            self.record_jit_pending(bytecode);
            *self.jit_compile_attempts.entry(bytecode_ptr).or_insert(0) += 1;
            if self
                .runtime_config
                .has_trace_bit(crate::config::trace_bits::JIT)
            {
                eprintln!(
                    "[jit] submitted background compilation: label={} bc_ptr={:#x} bclen={}",
                    closure.template.display_label(),
                    bytecode_ptr as usize,
                    closure.template.bytecode.len(),
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
                    // As in `poll_jit_completions`: no pin, no install — the
                    // key may already name a different function's bytecode.
                    let pin = self.jit_pending.remove(&result.bytecode_key);
                    match result.result {
                        Ok(jit_code) => {
                            let Some(pin) = pin else { continue };
                            self.install_jit_code(pin, Arc::new(jit_code));
                        }
                        Err(e) => {
                            let bytecode_ptr = result.bytecode_key as *const u8;
                            self.jit_rejections
                                .entry(bytecode_ptr)
                                .or_insert_with(|| JitRejectionInfo::new(e, pin));
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
            .is_some_and(|(b, _)| b.intersects(SIG_ERROR) || b.intersects(SIG_HALT))
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

            if self.enforce_squelch(sig, closure.squelch_mask) {
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            return Some(sig);
        }

        // Check for pending tail call (JIT function did a TailCall). The
        // resolved body is the tail callee's — hand it its executing-closure
        // register via the one-shot, as `trampoline_loop` does on a frame
        // replacement, so a self-reference in it resolves to the callee.
        if result == TAIL_CALL_SENTINEL {
            if let Some(tail) = self.pending_tail_call.take() {
                self.pending_entry_closure = tail.closure;
                let exec_result = self.execute_bytecode_saving_stack(&tail.code, &tail.env);
                let eb = exec_result.bits;
                if eb.is_empty() {
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
                } else if eb.intersects(SIG_ERROR) {
                    // SIG_ERROR — signal already set on fiber
                    self.fiber.stack.push(Value::NIL);
                    return None;
                } else {
                    // Suspending signal (SIG_FUEL, SIG_YIELD, SIG_SWITCH, user-defined).
                    // A non-yield suspend (fuel) leaves the tail callee's inner
                    // frame in exec_result.stack, not fiber.suspended; park it so
                    // resume re-enters the callee — a tail-recursive interpreter
                    // callee (e.g. `fold`) otherwise loses its accumulator across
                    // preemption (tests/elle/fuel-jit-preempt.lisp).
                    let mut frames = self.fiber.suspended.take().unwrap_or_default();
                    self.park_suspended_callee_frame(&mut frames, eb, exec_result);
                    self.fiber.suspended = Some(frames);
                    if self.enforce_squelch(eb, tail.squelch_mask | closure.squelch_mask) {
                        self.fiber.stack.push(Value::NIL);
                        return None;
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

        // Debug builds: detonate here, with attribution, if the env backing's
        // region was freed — compiled code would read it unchecked.
        crate::jit::dispatch::debug_check_env_backing(unsafe { &*self.heap_ptr }, closure);

        // Interpreter→JIT non-tail entry: hand the compiled callee one
        // `CallArgument` owning reference per non-captured fixed param, exactly
        // as the interpreter's `build_closure_env` (own_params=true) does for an
        // interpreter callee and `elle_jit_call` does for the JIT-to-JIT path.
        // The callee releases each owned param via `DecrefValueRegion`; without
        // this incref a heap arg held by the caller is over-released (UAF).
        crate::jit::dispatch::incref_owned_call_args(unsafe { &mut *self.heap_ptr }, closure, args);

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
}
