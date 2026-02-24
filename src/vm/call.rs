//! Call and TailCall instruction handlers.
//!
//! Handles:
//! - Native function calls (routes to signal dispatch in signal.rs)
//! - Closure calls with environment setup
//! - Yield-through-calls (suspended frame chain building)
//! - Tail call optimization
//! - JIT compilation and dispatch

use crate::value::error_val;
use crate::value::{SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
// SmallVec was tried here but benchmarks showed no improvement over Vec
// for the common 0-8 arg case. The inline storage (64 bytes) touches a
// full cache line regardless of arg count, and the is-inline branch on
// every push adds overhead that cancels out the allocation savings.
use std::rc::Rc;

use super::core::VM;

use crate::jit::{JitCode, JitCompiler, TAIL_CALL_SENTINEL};

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
    ) -> Option<SignalBits> {
        let bc: &[u8] = bytecode;
        let arg_count = self.read_u8(bc, ip) as usize;
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

        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(args.as_slice());
            return self.handle_primitive_signal(bits, value, bytecode, constants, closure_env, ip);
        }

        if let Some(closure) = func.as_closure() {
            self.fiber.call_depth += 1;
            if self.fiber.call_depth > 1000 {
                set_error(&mut self.fiber, "error", "Stack overflow");
                return Some(SIG_ERROR);
            }

            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // JIT compilation and dispatch — only for non-suspending closures
            // Suspending closures can never be JIT-compiled, so skip profiling overhead
            if !closure.effect.may_suspend() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    return bits;
                }
            }

            // Build the new environment
            let new_env_rc = self.build_closure_env(closure, &args);

            // Execute the closure, saving/restoring the caller's stack.
            // Essential for fiber/signal propagation and yield-through-nested-calls.
            let (bits, _ip) = self.execute_bytecode_saving_stack(
                &closure.bytecode,
                &closure.constants,
                &new_env_rc,
            );

            self.fiber.call_depth -= 1;

            match bits {
                SIG_OK => {
                    let (_, value) = self.fiber.signal.take().unwrap();
                    self.fiber.stack.push(value);
                }
                SIG_YIELD => {
                    // Yield propagated from a nested call. Two cases:
                    //
                    // 1. yield instruction: suspended frames exist — append the
                    //    caller's frame so resume replays the full call stack.
                    //
                    // 2. fiber/signal: no suspended frames — just propagate the
                    //    signal. The fiber saves its own context for resumption.
                    if let Some(mut frames) = self.fiber.suspended.take() {
                        let (_, value) = self.fiber.signal.take().unwrap();

                        let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                        let caller_frame = SuspendedFrame {
                            bytecode: bytecode.clone(),
                            constants: constants.clone(),
                            env: closure_env.clone(),
                            ip: *ip,
                            stack: caller_stack,
                        };

                        frames.push(caller_frame);
                        self.fiber.signal = Some((SIG_YIELD, value));
                        self.fiber.suspended = Some(frames);
                    }
                    return Some(SIG_YIELD);
                }
                _ => {
                    // Other signal (error, etc.) — propagate to caller.
                    return Some(bits);
                }
            }
            return None;
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
        let arg_count = self.read_u8(bytecode, ip) as usize;
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

        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(&args);
            return Some(self.handle_primitive_signal_tail(bits, value));
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Build proper environment using cached vector
            self.tail_call_env_cache.clear();
            let needed = closure.env_capacity();
            if self.tail_call_env_cache.capacity() < needed {
                self.tail_call_env_cache
                    .reserve(needed - self.tail_call_env_cache.len());
            }
            self.tail_call_env_cache
                .extend((*closure.env).iter().cloned());

            // Add parameters, handling variadic rest collection
            match closure.arity {
                crate::value::Arity::AtLeast(n) => {
                    for (i, arg) in args[..n].iter().enumerate() {
                        if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                            self.tail_call_env_cache.push(Value::local_cell(*arg));
                        } else {
                            self.tail_call_env_cache.push(*arg);
                        }
                    }
                    let rest = Self::args_to_list(&args[n..]);
                    let rest_idx = n;
                    if rest_idx < 64 && (closure.cell_params_mask & (1 << rest_idx)) != 0 {
                        self.tail_call_env_cache.push(Value::local_cell(rest));
                    } else {
                        self.tail_call_env_cache.push(rest);
                    }
                }
                _ => {
                    for (i, arg) in args.iter().enumerate() {
                        if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                            self.tail_call_env_cache.push(Value::local_cell(*arg));
                        } else {
                            self.tail_call_env_cache.push(*arg);
                        }
                    }
                }
            }

            // Calculate and add locally-defined variables
            let num_param_slots = match closure.arity {
                crate::value::Arity::Exact(n) => n,
                crate::value::Arity::AtLeast(n) => n + 1,
                crate::value::Arity::Range(min, _) => min,
            };
            let num_locally_defined = closure.num_locals.saturating_sub(num_param_slots);

            for _ in 0..num_locally_defined {
                self.tail_call_env_cache.push(Value::local_cell(Value::NIL));
            }

            let new_env_rc = Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information (Rc clones, not data copies)
            self.pending_tail_call = Some((
                closure.bytecode.clone(),
                closure.constants.clone(),
                new_env_rc,
            ));

            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        Some(SIG_ERROR)
    }

    // ── JIT ─────────────────────────────────────────────────────────

    /// Try JIT compilation/dispatch for a closure call.
    ///
    /// Returns `Some(Option<SignalBits>)` if JIT handled the call (the inner
    /// Option follows handle_call's convention), or `None` to fall through
    /// to the interpreter path. Caller is responsible for decrementing
    /// call_depth on the `Some` path.
    fn try_jit_call(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
        func: Value,
    ) -> Option<Option<SignalBits>> {
        let bytecode_ptr = closure.bytecode.as_ptr();
        let is_hot = self.record_closure_call(bytecode_ptr);

        // Check if we already have JIT code for this closure
        if let Some(jit_code) = self.jit_cache.get(&bytecode_ptr).cloned() {
            return Some(self.run_jit(&jit_code, closure, args, func));
        }

        // If hot, attempt JIT compilation
        if is_hot {
            if let Some(ref lir_func) = closure.lir_function {
                match JitCompiler::new() {
                    Ok(compiler) => match compiler.compile(lir_func) {
                        Ok(jit_code) => {
                            let jit_code = Rc::new(jit_code);
                            self.jit_cache.insert(bytecode_ptr, jit_code.clone());
                            return Some(self.run_jit(&jit_code, closure, args, func));
                        }
                        Err(e) => match &e {
                            crate::jit::JitError::UnsupportedInstruction(_) => {
                                // MakeClosure and other instructions not yet in JIT.
                                // Fall back to interpreter — the function still works.
                            }
                            _ => {
                                panic!(
                                    "JIT compilation failed for pure function: {}. \
                                     This is a bug — pure functions should be JIT-compilable. \
                                     Error: {}",
                                    closure
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
        if matches!(self.fiber.signal, Some((SIG_ERROR | SIG_HALT, _))) {
            self.fiber.stack.push(Value::NIL);
            return None; // Let the dispatch loop's signal check deal with it
        }

        // Check for pending tail call (JIT function did a TailCall)
        if result.to_bits() == TAIL_CALL_SENTINEL {
            if let Some((tail_bc, tail_consts, tail_env)) = self.pending_tail_call.take() {
                match self.execute_closure_bytecode(&tail_bc, &tail_consts, &tail_env) {
                    Ok(val) => {
                        self.fiber.stack.push(val);
                        return None;
                    }
                    Err(e) => {
                        set_error(&mut self.fiber, "error", e);
                        self.fiber.stack.push(Value::NIL);
                        return None;
                    }
                }
            }
        }

        self.fiber.stack.push(result);
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
    ///
    /// Uses zero-copy pointer casts for both env and args since Value is
    /// `#[repr(transparent)]` over u64.
    fn call_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
        func_value: Value,
    ) -> Value {
        // Zero-copy: Value is #[repr(transparent)] over u64, so &[Value]
        // has the same layout as &[u64]. Cast pointers directly.
        let env_ptr = if closure.env.is_empty() {
            std::ptr::null()
        } else {
            closure.env.as_ptr() as *const u64
        };

        let result_bits = unsafe {
            jit_code.call(
                env_ptr,
                args.as_ptr() as *const u64,
                args.len() as u32,
                self as *mut VM as *mut (),
                func_value.to_bits(),
            )
        };

        unsafe { Value::from_bits(result_bits) }
    }

    // ── Environment building ────────────────────────────────────────

    /// Build a closure environment from captured variables and arguments.
    ///
    /// Reuses `self.env_cache` to avoid a fresh Vec allocation per call.
    pub(super) fn build_closure_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Rc<Vec<Value>> {
        self.env_cache.clear();
        let needed = closure.env_capacity();
        if self.env_cache.capacity() < needed {
            self.env_cache.reserve(needed - self.env_cache.len());
        }
        self.env_cache.extend((*closure.env).iter().cloned());

        match closure.arity {
            crate::value::Arity::AtLeast(n) => {
                // Variadic: first n args are fixed params, rest collected into a list
                for (i, arg) in args[..n].iter().enumerate() {
                    if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                        self.env_cache.push(Value::local_cell(*arg));
                    } else {
                        self.env_cache.push(*arg);
                    }
                }
                // Collect remaining args into a list for the rest slot
                let rest = Self::args_to_list(&args[n..]);
                let rest_idx = n; // rest param is at index n in the param list
                if rest_idx < 64 && (closure.cell_params_mask & (1 << rest_idx)) != 0 {
                    self.env_cache.push(Value::local_cell(rest));
                } else {
                    self.env_cache.push(rest);
                }
            }
            _ => {
                // Fixed arity: all args are direct params
                for (i, arg) in args.iter().enumerate() {
                    if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                        self.env_cache.push(Value::local_cell(*arg));
                    } else {
                        self.env_cache.push(*arg);
                    }
                }
            }
        }

        // Calculate number of locally-defined variables
        let num_param_slots = match closure.arity {
            crate::value::Arity::Exact(n) => n,
            crate::value::Arity::AtLeast(n) => n + 1, // fixed + rest slot
            crate::value::Arity::Range(min, _) => min,
        };
        let num_locally_defined = closure.num_locals.saturating_sub(num_param_slots);

        // Add empty LocalCells for locally-defined variables
        for _ in 0..num_locally_defined {
            self.env_cache.push(Value::local_cell(Value::NIL));
        }

        Rc::new(self.env_cache.clone())
    }

    /// Collect values into an Elle list (cons chain terminated by EMPTY_LIST).
    fn args_to_list(args: &[Value]) -> Value {
        let mut list = Value::EMPTY_LIST;
        for arg in args.iter().rev() {
            list = Value::cons(*arg, list);
        }
        list
    }
}
