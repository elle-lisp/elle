//! Call and TailCall instruction handlers.
//!
//! Handles:
//! - Native function calls (routes to signal dispatch in signal.rs)
//! - Closure calls with environment setup
//! - Yield-through-calls (suspended frame chain building)
//! - Tail call optimization
//!
//! Environment building (closure env population, parameter binding) lives in `env.rs`.

use crate::error::LocationMap;
use crate::primitives::access::resolve_index;
use crate::value::error_val;
use crate::value::fiber::CallFrame;
use crate::value::{
    BytecodeFrame, SignalBits, SuspendedFrame, TableKey, Value, SIG_ERROR, SIG_HALT, SIG_OK,
    SIG_SWITCH,
};
// SmallVec was tried here but benchmarks showed no improvement over Vec
// for the common 0-8 arg case. The inline storage (64 bytes) touches a
// full cache line regardless of arg count, and the is-inline branch on
// every push adds overhead that cancels out the allocation savings.
use std::rc::Rc;

use super::core::VM;

/// Helper: set an error signal on the fiber.
fn set_error(fiber: &mut crate::value::Fiber, kind: &str, msg: impl Into<String>) {
    fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
}

impl VM {
    /// Handle the Call instruction.
    ///
    /// Pops the function and arguments from the stack, calls the function,
    /// and pushes the result. Handles native functions, VM-aware functions,
    /// and closures with proper environment setup.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_call(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        let bc: &[u8] = bytecode;
        let arg_count = self.read_u16(bc, ip) as usize;
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on Call");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on Call"),
            );
        }
        args.reverse();

        self.call_inner(
            func,
            args,
            bytecode,
            constants,
            closure_env,
            ip,
            instr_ip,
            location_map,
        )
    }

    /// Handle the CallArrayMut instruction.
    ///
    /// Like Call, but instead of reading arg_count from bytecode and popping
    /// individual args, pops an args array and uses its elements as arguments.
    /// Used by splice: the lowerer builds an args array, then CallArrayMut
    /// calls the function with those args.
    ///
    /// Stack: \[func, args_array\] → \[result\]
    pub(super) fn handle_call_array(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            self.fiber.stack.push(Value::NIL);
            return None;
        };

        self.call_inner(
            func,
            args,
            bytecode,
            constants,
            closure_env,
            ip,
            instr_ip,
            location_map,
        )
    }

    /// Shared Call/CallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions, executes closures with environment setup,
    /// handles yield-through-calls and JIT compilation.
    #[allow(clippy::too_many_arguments)]
    fn call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        if let Some(def) = func.as_native_def() {
            etrace!(
                self,
                crate::config::trace_bits::CALL,
                "call",
                "native {} nargs={}",
                def.name,
                args.len()
            );
            let blocked = def
                .signal
                .bits
                .intersection(self.fiber.withheld)
                .intersection(crate::signals::CAP_MASK);
            if !blocked.is_empty() {
                return self.handle_capability_denial(
                    def,
                    blocked,
                    &args,
                    bytecode,
                    constants,
                    closure_env,
                    ip,
                    location_map,
                );
            }
            let (bits, value) = (def.func)(args.as_slice());
            return self.handle_primitive_signal(
                bits,
                value,
                bytecode,
                constants,
                closure_env,
                ip,
                location_map,
            );
        }

        if let Some((id, default)) = func.as_parameter() {
            if !args.is_empty() {
                set_error(
                    &mut self.fiber,
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
            let value = self.resolve_parameter(id, default);
            self.fiber.stack.push(value);
            return None;
        }

        if let Some(closure) = func.as_closure() {
            etrace!(
                self,
                crate::config::trace_bits::CALL,
                "call",
                "closure {} nargs={}",
                closure.template.name.as_deref().unwrap_or("<anon>"),
                args.len()
            );
            self.fiber.call_depth += 1;

            // Push call frame for stack traces
            self.fiber.call_stack.push(CallFrame {
                name: closure
                    .template
                    .name
                    .clone()
                    .unwrap_or_else(|| Rc::from("<anonymous>")),
                ip: instr_ip,
                frame_base: 0, // Closures always execute with fresh stack via execute_bytecode_saving_stack
                location_map: location_map.clone(),
            });

            // Validate argument count
            if !self.check_arity(&closure.template.arity, args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.call_stack.pop();
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // Tiered WASM compilation and dispatch.
            // Checked before JIT because WASM is the preferred fast path when enabled.
            if closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_wasm_call(closure, &args) {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
                    return bits;
                }
            }

            // JIT compilation and dispatch.
            // Polymorphic closures are rejected by the JIT compiler itself.
            // Skip profiling for primitives (no LIR means not JIT-compilable).
            if closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    match bits {
                        Some(sig) if !sig.contains(SIG_ERROR) && !sig.contains(SIG_HALT) => {
                            // JIT function suspended — any bits except SIG_ERROR/SIG_HALT
                            // cause the caller frame to be appended for resumption.
                            // fiber.signal and fiber.suspended are set by the JIT yield
                            // helpers. Build the interpreter-level caller frame.
                            // Use unwrap_or_default() so this works whether the JIT callee
                            // populated fiber.suspended or not (tail-call-to-native path).
                            {
                                let (_, value) = self.fiber.signal.take().unwrap();
                                let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                                let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
                                    bytecode: bytecode.clone(),
                                    constants: constants.clone(),
                                    env: closure_env.clone(),
                                    ip: *ip,
                                    stack: caller_stack,
                                    location_map: location_map.clone(),
                                    // Caller frame: on resume, the callee's return value
                                    // flows as current_value and must be pushed as the
                                    // Call instruction's result.
                                    push_resume_value: true,
                                });
                                let mut frames = self.fiber.suspended.take().unwrap_or_default();
                                frames.push(caller_frame);
                                self.fiber.signal = Some((sig, value));
                                self.fiber.suspended = Some(frames);
                            }
                            return Some(sig);
                        }
                        other => return other,
                    }
                }
            }

            // Build the new environment
            let new_env_rc = match self.build_closure_env(closure, &args) {
                Some(env) => env,
                None => {
                    self.fiber.call_depth -= 1;
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            };

            // Extract squelch_mask before execute_bytecode_saving_stack to avoid
            // borrow lifetime conflicts: `closure` borrows from `func`, and we
            // need `closure_squelch_mask` after the call returns.
            let closure_squelch_mask = closure.squelch_mask;

            // Guard: WASM-compiled closures have empty bytecode. They
            // cannot be executed by the bytecode VM.
            if closure.template.bytecode.is_empty() {
                let err = crate::value::error_val(
                    "exec-error",
                    "cannot execute WASM closure in bytecode VM",
                );
                self.fiber.stack.push(err);
                self.fiber.call_depth -= 1;
                return Some(SIG_ERROR);
            }

            // Execute the closure, saving/restoring the caller's stack.
            // Essential for fiber/signal propagation and yield-through-nested-calls.
            let result = self.execute_bytecode_saving_stack(
                &closure.template.bytecode,
                &closure.template.constants,
                &new_env_rc,
                &closure.template.location_map,
            );

            self.fiber.call_depth -= 1;

            let bits = result.bits;

            // Squelch enforcement: if the closure has a squelch mask and the callee
            // returned a non-OK, non-error, non-halt signal that matches the mask,
            // convert to a signal-violation error.
            //
            // We do NOT intercept SIG_ERROR (already an error) or SIG_HALT (terminal).
            // We DO intercept SIG_YIELD and user-defined signals.
            //
            // Note: do_fiber_first_resume is intentionally exempt — fiber root bodies
            // execute outside any call_inner, so squelch enforcement does not apply
            // to the initial fiber execution.
            //
            // Discard suspended frames: we're converting to error, not suspending.
            if !closure_squelch_mask.is_empty()
                && !bits.is_ok()
                && !bits.contains(SIG_ERROR)
                && !bits.contains(SIG_HALT)
                && bits != SIG_SWITCH
            {
                let squelched = bits.intersection(closure_squelch_mask);
                if !squelched.is_empty() {
                    let squelched_str = {
                        let registry = crate::signals::registry::global_registry().lock().unwrap();
                        registry.format_signal_bits(squelched)
                    };
                    let err = crate::value::error_val(
                        "signal-violation",
                        format!("squelch: signal {} caught at boundary", squelched_str),
                    );
                    // Discard suspended frames — we're converting to error, not suspending.
                    self.fiber.suspended = None;
                    self.fiber.signal = Some((SIG_ERROR, err));
                    self.fiber.call_stack.pop();
                    return Some(SIG_ERROR);
                }
            }
            if bits.is_ok() {
                let (_, value) = self.fiber.signal.take().unwrap();
                self.fiber.stack.push(value);
                self.fiber.call_stack.pop();
            } else if !bits.contains(SIG_ERROR) && !bits.contains(SIG_HALT) {
                // Suspending signal — any bits except SIG_ERROR/SIG_HALT
                // cause the caller frame to be appended for resumption.
                // Propagated from a nested call (interpreter or tail-call-to-native path).
                // We must always build the caller frame, whether or not the callee
                // already populated fiber.suspended. When the callee is a TailCall to
                // a native yielding primitive, it does NOT create a SuspendedFrame
                // (TCO), so fiber.suspended may be None here — use unwrap_or_default()
                // to cover both cases.
                {
                    let (_, value) = self.fiber.signal.take().unwrap();

                    let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::CALL)
                        && caller_stack.len() <= 5
                    {
                        eprintln!(
                            "[call_inner suspend] ip={} bc_len={} stack_depth={}",
                            *ip,
                            bytecode.len(),
                            caller_stack.len(),
                        );
                        for (si, sv) in caller_stack.iter().enumerate() {
                            eprintln!("  stack[{}] = {} {:?}", si, sv.type_name(), sv);
                        }
                    }
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
                        bytecode: bytecode.clone(),
                        constants: constants.clone(),
                        env: closure_env.clone(),
                        ip: *ip,
                        stack: caller_stack,
                        location_map: location_map.clone(),
                        push_resume_value: true,
                    });

                    let mut frames = self.fiber.suspended.take().unwrap_or_default();
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::FIBER)
                    {
                        eprintln!(
                            "[call_inner] suspend: bits={} ip={} bc_len={} inner_frames={} env_len={}",
                            bits, *ip, bytecode.len(), frames.len(), closure_env.len(),
                        );
                    }
                    frames.push(caller_frame);
                    self.fiber.signal = Some((bits, value));
                    self.fiber.suspended = Some(frames);
                }
                self.fiber.call_stack.pop();
                return Some(bits);
            } else {
                // Other signal (error, etc.) — propagate to caller.
                // The call frame is preserved on error for stack traces.
                return Some(bits);
            }
            return None;
        }

        // Callable collections: struct, array, set
        if let Some(result) = call_collection(&func, &args) {
            match result {
                Ok(value) => {
                    self.fiber.stack.push(value);
                    return None;
                }
                Err((kind, msg)) => {
                    set_error(&mut self.fiber, kind, msg);
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            }
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        self.fiber.stack.push(Value::NIL);
        None
    }

    /// Handle the TailCall instruction.
    ///
    /// Similar to Call but sets up a pending tail call instead of recursing,
    /// enabling tail call optimization.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_tail_call(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Option<SignalBits> {
        let arg_count = self.read_u16(bytecode, ip) as usize;
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCall");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on TailCall"),
            );
        }
        args.reverse();

        self.tail_call_inner(func, args)
    }

    /// Handle the TailCallArrayMut instruction.
    ///
    /// Like TailCall, but pops an args array instead of individual args.
    /// Stack: \[func, args_array\] → (sets up pending tail call)
    pub(super) fn handle_tail_call_array(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Option<SignalBits> {
        // Suppress unused warnings — these params match the dispatch signature
        let _ = (ip, bytecode);

        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            return Some(SIG_ERROR);
        };

        self.tail_call_inner(func, args)
    }

    /// Shared TailCall/TailCallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions via tail signal handler, sets up pending
    /// tail call for closures with environment building.
    fn tail_call_inner(&mut self, func: Value, args: Vec<Value>) -> Option<SignalBits> {
        if let Some(def) = func.as_native_def() {
            let blocked = def
                .signal
                .bits
                .intersection(self.fiber.withheld)
                .intersection(crate::signals::CAP_MASK);
            if !blocked.is_empty() {
                return Some(self.handle_capability_denial_tail(def, blocked, &args));
            }
            let (bits, value) = (def.func)(&args);
            return Some(self.handle_primitive_signal_tail(bits, value));
        }

        if let Some((id, default)) = func.as_parameter() {
            if !args.is_empty() {
                set_error(
                    &mut self.fiber,
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                return Some(SIG_ERROR);
            }
            let value = self.resolve_parameter(id, default);
            self.fiber.signal = Some((SIG_OK, value));
            return Some(SIG_OK);
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count
            if !self.check_arity(&closure.template.arity, args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Build proper environment using cached vector
            if !Self::populate_env(
                &mut self.tail_call_env_cache,
                &mut self.fiber,
                closure,
                &args,
            ) {
                return Some(SIG_ERROR);
            }
            let new_env_rc = Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information (Rc clones, not data copies)
            self.pending_tail_call = Some(crate::vm::core::TailCallInfo {
                bytecode: closure.template.bytecode.clone(),
                constants: closure.template.constants.clone(),
                env: new_env_rc,
                location_map: closure.template.location_map.clone(),
                rotation_safe: closure.template.rotation_safe,
                squelch_mask: closure.squelch_mask,
            });

            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Callable collections: struct, array, set
        if let Some(result) = call_collection(&func, &args) {
            match result {
                Ok(value) => {
                    self.fiber.signal = Some((SIG_OK, value));
                    return Some(SIG_OK);
                }
                Err((kind, msg)) => {
                    set_error(&mut self.fiber, kind, msg);
                    return Some(SIG_ERROR);
                }
            }
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        Some(SIG_ERROR)
    }

    /// Call a compiled closure with the given argument values.
    ///
    /// Used by macro expansion to invoke cached transformer closures
    /// without going through the full `eval_syntax` pipeline.
    ///
    /// Returns the closure's return value on success. Returns `Err` on
    /// arity mismatch, error signal, or halt.
    ///
    /// Callers must not pass closures that may yield (signal includes
    /// `SIG_YIELD`). Macro transformer closures are always silent.
    pub fn call_closure(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Result<Value, String> {
        // Arity check — sets fiber.signal on mismatch.
        if !self.check_arity(&closure.template.arity, args.len()) {
            let (_, err) = self.fiber.signal.take().unwrap();
            return Err(self.format_error_with_location(err));
        }

        // Build the closure environment (captures + param slots + local slots).
        let new_env = match self.build_closure_env(closure, args) {
            Some(env) => env,
            None => {
                let (_, err) = self.fiber.signal.take().unwrap();
                return Err(self.format_error_with_location(err));
            }
        };

        // Execute the closure bytecode, saving/restoring the caller's stack.
        let result = self.execute_bytecode_saving_stack(
            &closure.template.bytecode,
            &closure.template.constants,
            &new_env,
            &closure.template.location_map,
        );

        let bits = result.bits;
        if bits.is_ok() || bits == crate::value::SIG_HALT {
            let (_, value) = self.fiber.signal.take().unwrap();
            Ok(value)
        } else if bits.contains(crate::value::SIG_ERROR) {
            let (_, err) = self
                .fiber
                .signal
                .take()
                .unwrap_or((crate::value::SIG_ERROR, Value::NIL));
            Err(self.format_error_with_location(err))
        } else {
            // Unexpected suspending signal (yield from macro body — not supported).
            self.fiber.signal.take();
            Err(format!(
                "Unexpected signal from macro transformer: {}",
                bits
            ))
        }
    }
}

/// Dispatch a call on a collection value.
///
/// Returns `None` if the value is not a callable collection.
/// Returns `Some(Ok(value))` on success.
/// Returns `Some(Err((kind, msg)))` on error.
pub(crate) fn call_collection(
    func: &Value,
    args: &[Value],
) -> Option<Result<Value, (&'static str, String)>> {
    // ── Structs (immutable and mutable) ──────────────────────────────
    if let Some(s) = func.as_struct() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("struct call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let key = match TableKey::from_value(&args[0]) {
            Some(k) => k,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "struct call: expected hashable key, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return Some(Ok(s.get(&key).copied().unwrap_or(default)));
    }
    if let Some(s) = func.as_struct_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@struct call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let key = match TableKey::from_value(&args[0]) {
            Some(k) => k,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@struct call: expected hashable key, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return Some(Ok(s.borrow().get(&key).copied().unwrap_or(default)));
    }

    // ── Arrays (immutable and mutable) ───────────────────────────────
    if let Some(elems) = func.as_array() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("array call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "array call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        match resolve_index(index, elems.len()) {
            Some(i) => return Some(Ok(elems[i])),
            None => return Some(Ok(default)),
        }
    }
    if let Some(vec_ref) = func.as_array_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@array call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@array call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = vec_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return Some(Ok(borrowed[i])),
            None => return Some(Ok(default)),
        }
    }

    // ── Strings (immutable and mutable) ────────────────────────────────
    if func.is_string() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("string call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "string call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        return func
            .with_string(|s| {
                use unicode_segmentation::UnicodeSegmentation;
                if index >= 0 {
                    match s.graphemes(true).nth(index as usize) {
                        Some(g) => Some(Ok(Value::string(g))),
                        None => Some(Ok(default)),
                    }
                } else {
                    let graphemes: Vec<&str> = s.graphemes(true).collect();
                    match resolve_index(index, graphemes.len()) {
                        Some(i) => Some(Ok(Value::string(graphemes[i]))),
                        None => Some(Ok(default)),
                    }
                }
            })
            .unwrap();
    }
    if let Some(buf_ref) = func.as_string_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@string call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@string call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = buf_ref.borrow();
        let s = match std::str::from_utf8(&borrowed) {
            Ok(s) => s,
            Err(e) => {
                return Some(Err((
                    "encoding-error",
                    format!("@string call: invalid UTF-8: {}", e),
                )))
            }
        };
        use unicode_segmentation::UnicodeSegmentation;
        if index >= 0 {
            return match s.graphemes(true).nth(index as usize) {
                Some(g) => Some(Ok(Value::string(g))),
                None => Some(Ok(default)),
            };
        } else {
            let graphemes: Vec<&str> = s.graphemes(true).collect();
            return match resolve_index(index, graphemes.len()) {
                Some(i) => Some(Ok(Value::string(graphemes[i]))),
                None => Some(Ok(default)),
            };
        }
    }

    // ── Bytes (immutable and mutable) ────────────────────────────────
    if let Some(b) = func.as_bytes() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("bytes call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "bytes call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        match resolve_index(index, b.len()) {
            Some(i) => return Some(Ok(Value::int(b[i] as i64))),
            None => return Some(Ok(default)),
        }
    }
    if let Some(blob_ref) = func.as_bytes_mut() {
        if args.is_empty() || args.len() > 2 {
            return Some(Err((
                "arity-error",
                format!("@bytes call: expected 1-2 arguments, got {}", args.len()),
            )));
        }
        let index = match args[0].as_int() {
            Some(i) => i,
            None => {
                return Some(Err((
                    "type-error",
                    format!(
                        "@bytes call: index must be integer, got {}",
                        args[0].type_name()
                    ),
                )))
            }
        };
        let default = if args.len() == 2 { args[1] } else { Value::NIL };
        let borrowed = blob_ref.borrow();
        match resolve_index(index, borrowed.len()) {
            Some(i) => return Some(Ok(Value::int(borrowed[i] as i64))),
            None => return Some(Ok(default)),
        }
    }

    // ── Sets (immutable and mutable) ─────────────────────────────────
    if let Some(s) = func.as_set() {
        if args.len() != 1 {
            return Some(Err((
                "arity-error",
                format!("set call: expected 1 argument, got {}", args.len()),
            )));
        }
        let frozen = crate::primitives::sets::freeze_value(args[0]);
        return Some(Ok(Value::bool(s.contains(&frozen))));
    }
    if let Some(s) = func.as_set_mut() {
        if args.len() != 1 {
            return Some(Err((
                "arity-error",
                format!("@set call: expected 1 argument, got {}", args.len()),
            )));
        }
        let frozen = crate::primitives::sets::freeze_value(args[0]);
        return Some(Ok(Value::bool(s.borrow().contains(&frozen))));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::register_primitives;
    use crate::symbol::SymbolTable;
    use crate::value::Value;

    fn make_vm_with_primitives() -> (VM, SymbolTable) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        (vm, symbols)
    }

    /// Verify that call_closure with a trivial identity closure returns the argument.
    #[test]
    fn test_call_closure_identity() {
        use crate::pipeline::eval_syntax;
        use crate::syntax::Expander;

        let (mut vm, mut symbols) = make_vm_with_primitives();
        let mut expander = Expander::new();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // Compile (fn (x) x) to a closure
        let syntax = crate::reader::read_syntax("(fn (x) x)", "<test>").unwrap();
        let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
        let closure = closure_val.as_closure().expect("should be a closure");

        let arg = Value::int(42);
        let result = vm.call_closure(closure, &[arg]).unwrap();
        assert_eq!(result, Value::int(42));
    }

    /// Verify that call_closure propagates errors from the closure body.
    #[test]
    fn test_call_closure_error_propagation() {
        use crate::pipeline::eval_syntax;
        use crate::syntax::Expander;

        let (mut vm, mut symbols) = make_vm_with_primitives();
        let mut expander = Expander::new();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // Compile (fn () (error "boom")) — always errors
        let syntax = crate::reader::read_syntax(r#"(fn () (error "boom"))"#, "<test>").unwrap();
        let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
        let closure = closure_val.as_closure().expect("should be a closure");

        let result = vm.call_closure(closure, &[]);
        assert!(result.is_err(), "should propagate error from closure body");
    }

    /// Counterfactual: verify the identity test assertion fires if we break it.
    #[test]
    #[ignore = "counterfactual — run manually to verify assertion strength"]
    fn test_call_closure_counterfactual() {
        use crate::pipeline::eval_syntax;
        use crate::syntax::Expander;

        let (mut vm, mut symbols) = make_vm_with_primitives();
        let mut expander = Expander::new();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syntax = crate::reader::read_syntax("(fn (x) x)", "<test>").unwrap();
        let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
        let closure = closure_val.as_closure().expect("should be a closure");

        let result = vm.call_closure(closure, &[Value::int(42)]).unwrap();
        // This should fail — intentionally wrong:
        assert_eq!(result, Value::int(99), "counterfactual: should fail here");
    }
}
