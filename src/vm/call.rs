//! Call and TailCall instruction handlers.
//!
//! These are the most complex instructions in the VM, handling:
//! - Native function calls
//! - VM-aware function calls
//! - Closure calls with environment setup
//! - Coroutine-aware execution
//! - Tail call optimization
//! - JIT compilation and dispatch (when jit feature is enabled)

use crate::value::{CoroutineState, Value};
use std::rc::Rc;

use super::core::{VmResult, VM};

#[cfg(feature = "jit")]
use crate::jit::{JitCode, JitCompiler};

impl VM {
    /// Handle the Call instruction.
    ///
    /// Pops the function and arguments from the stack, calls the function,
    /// and pushes the result. Handles native functions, VM-aware functions,
    /// and closures with proper environment setup.
    pub(super) fn handle_call(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        ip: &mut usize,
    ) -> Result<Option<VmResult>, String> {
        let arg_count = self.read_u8(bytecode, ip) as usize;
        let func = self.stack.pop().ok_or("Stack underflow")?;

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.stack.pop().ok_or("Stack underflow")?);
        }
        args.reverse();

        if let Some(f) = func.as_native_fn() {
            let result = match f(args.as_slice()) {
                Ok(val) => val,
                Err(cond) => {
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    Value::NIL
                }
            };
            self.stack.push(result);
            return Ok(None);
        }

        if let Some(f) = func.as_vm_aware_fn() {
            let result = f(args.as_slice(), self)?;
            self.stack.push(result);

            // Check for pending yield from yield-from delegation
            if let Some(yielded_value) = self.take_pending_yield() {
                return self.handle_pending_yield_after_call(
                    bytecode,
                    constants,
                    closure_env,
                    *ip,
                    yielded_value,
                );
            }
            return Ok(None);
        }

        if let Some(closure) = func.as_closure() {
            self.call_depth += 1;
            if self.call_depth > 1000 {
                return Err("Stack overflow".to_string());
            }

            // Record this closure call for profiling
            let bytecode_ptr = closure.bytecode.as_ptr();
            let is_hot = self.record_closure_call(bytecode_ptr);

            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                self.call_depth -= 1;
                self.stack.push(Value::NIL);
                return Ok(None);
            }

            // JIT compilation and dispatch
            #[cfg(feature = "jit")]
            {
                // Check if we already have JIT code for this closure
                if let Some(jit_code) = self.jit_cache.get(&bytecode_ptr).cloned() {
                    let result = self.call_jit(&jit_code, closure, &args);
                    self.call_depth -= 1;
                    self.stack.push(result);
                    return Ok(None);
                }

                // If hot and pure, attempt JIT compilation
                if is_hot && closure.effect.is_pure() {
                    if let Some(ref lir_func) = closure.lir_function {
                        match JitCompiler::new() {
                            Ok(compiler) => {
                                match compiler.compile(lir_func) {
                                    Ok(jit_code) => {
                                        let jit_code = Rc::new(jit_code);
                                        // Cache the JIT code
                                        self.jit_cache.insert(bytecode_ptr, jit_code.clone());
                                        // Execute via JIT
                                        let result = self.call_jit(&jit_code, closure, &args);
                                        self.call_depth -= 1;
                                        self.stack.push(result);
                                        return Ok(None);
                                    }
                                    Err(_e) => {
                                        // JIT compilation failed — fall back to interpreter
                                        // Could log: eprintln!("JIT compilation failed: {}", _e);
                                    }
                                }
                            }
                            Err(_) => {
                                // JIT compiler creation failed — fall back to interpreter
                            }
                        }
                    }
                }
            }

            // Suppress unused variable warning when JIT is disabled
            #[cfg(not(feature = "jit"))]
            let _ = is_hot;

            // Build the new environment
            let new_env_rc = self.build_closure_env(closure, &args);

            // Execute the closure (interpreter path)
            if self.in_coroutine() {
                let result = self.execute_bytecode_coroutine(
                    &closure.bytecode,
                    &closure.constants,
                    Some(&new_env_rc),
                )?;

                self.call_depth -= 1;

                match result {
                    VmResult::Done(v) => {
                        self.stack.push(v);
                    }
                    VmResult::Yielded {
                        value,
                        continuation,
                    } => {
                        // Capture the caller's frame and append it to the continuation
                        let caller_stack: Vec<Value> = self.stack.drain(..).collect();

                        let caller_frame = crate::value::ContinuationFrame {
                            bytecode: Rc::new(bytecode.to_vec()),
                            constants: Rc::new(constants.to_vec()),
                            env: closure_env.cloned().unwrap_or_else(|| Rc::new(vec![])),
                            ip: *ip,
                            stack: caller_stack,
                            exception_handlers: self.exception_handlers.clone(),
                            handling_exception: self.handling_exception,
                        };

                        let mut cont_data = continuation
                            .as_continuation()
                            .expect("Yielded continuation must be a continuation value")
                            .as_ref()
                            .clone();
                        cont_data.append_frame(caller_frame);

                        let new_continuation = Value::continuation(cont_data);
                        return Ok(Some(VmResult::Yielded {
                            value,
                            continuation: new_continuation,
                        }));
                    }
                }
            } else {
                let result = self.execute_bytecode(
                    &closure.bytecode,
                    &closure.constants,
                    Some(&new_env_rc),
                )?;

                self.call_depth -= 1;
                self.stack.push(result);
            }
            return Ok(None);
        }

        Err(format!("Cannot call {:?}", func))
    }

    /// Handle the TailCall instruction.
    ///
    /// Similar to Call but sets up a pending tail call instead of recursing,
    /// enabling tail call optimization.
    pub(super) fn handle_tail_call(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Result<Option<VmResult>, String> {
        let arg_count = self.read_u8(bytecode, ip) as usize;
        let func = self.stack.pop().ok_or("Stack underflow")?;

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.stack.pop().ok_or("Stack underflow")?);
        }
        args.reverse();

        if let Some(f) = func.as_native_fn() {
            match f(&args) {
                Ok(val) => return Ok(Some(VmResult::Done(val))),
                Err(cond) => {
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return Ok(Some(VmResult::Done(Value::NIL)));
                }
            }
        }

        if let Some(f) = func.as_vm_aware_fn() {
            return f(&args, self)
                .map(|v| Some(VmResult::Done(v)))
                .map_err(|e| e.description());
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count
            if !self.check_arity(&closure.arity, args.len()) {
                if self.current_exception.is_some()
                    && !self.handling_exception
                    && self.exception_handlers.is_empty()
                {
                    return Ok(Some(VmResult::Done(Value::NIL)));
                }
                return Ok(Some(VmResult::Done(Value::NIL)));
            }

            // Build proper environment using cached vector
            self.tail_call_env_cache.clear();
            self.tail_call_env_cache
                .extend((*closure.env).iter().cloned());

            // Add parameters, wrapping in local cells if needed
            for (i, arg) in args.iter().enumerate() {
                if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                    self.tail_call_env_cache.push(Value::local_cell(*arg));
                } else {
                    self.tail_call_env_cache.push(*arg);
                }
            }

            // Calculate and add locally-defined variables
            let num_params = match closure.arity {
                crate::value::Arity::Exact(n) => n,
                crate::value::Arity::AtLeast(n) => n,
                crate::value::Arity::Range(min, _) => min,
            };
            let num_locally_defined = closure.num_locals.saturating_sub(num_params);

            for _ in 0..num_locally_defined {
                self.tail_call_env_cache.push(Value::local_cell(Value::NIL));
            }

            let new_env_rc = std::rc::Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information
            self.pending_tail_call = Some((
                (*closure.bytecode).clone(),
                (*closure.constants).clone(),
                new_env_rc,
            ));

            return Ok(Some(VmResult::Done(Value::NIL)));
        }

        Err(format!("Cannot call {:?}", func))
    }

    /// Call a JIT-compiled function.
    ///
    /// # Safety
    /// The JIT code must have been compiled from the same LIR function that
    /// produced the closure's bytecode. The calling convention must match.
    #[cfg(feature = "jit")]
    fn call_jit(
        &mut self,
        jit_code: &JitCode,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Value {
        // Convert args to bits for the JIT calling convention
        // We need to pass Value bits, not Value pointers
        let args_bits: Vec<u64> = args.iter().map(|v| v.to_bits()).collect();

        // Get environment pointer (captures)
        // The JIT expects a pointer to an array of Value bits (u64)
        let env_bits: Vec<u64> = closure.env.iter().map(|v| v.to_bits()).collect();
        let env_ptr = if env_bits.is_empty() {
            std::ptr::null()
        } else {
            env_bits.as_ptr()
        };

        // Call the JIT-compiled function
        let result_bits = unsafe {
            jit_code.call(
                env_ptr,
                args_bits.as_ptr(),
                args.len() as u32,
                self as *mut VM as *mut (),
            )
        };

        // Convert result back to Value
        unsafe { Value::from_bits(result_bits) }
    }

    /// Build a closure environment from captured variables and arguments.
    fn build_closure_env(&self, closure: &crate::value::Closure, args: &[Value]) -> Rc<Vec<Value>> {
        let mut new_env = Vec::new();
        new_env.extend((*closure.env).iter().cloned());

        // Add parameters, wrapping in local cells if cell_params_mask indicates
        for (i, arg) in args.iter().enumerate() {
            if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                new_env.push(Value::local_cell(*arg));
            } else {
                new_env.push(*arg);
            }
        }

        // Calculate number of locally-defined variables
        let num_params = match closure.arity {
            crate::value::Arity::Exact(n) => n,
            crate::value::Arity::AtLeast(n) => n,
            crate::value::Arity::Range(min, _) => min,
        };
        let num_locally_defined = closure.num_locals.saturating_sub(num_params);

        // Add empty LocalCells for locally-defined variables
        for _ in 0..num_locally_defined {
            new_env.push(Value::local_cell(Value::NIL));
        }

        Rc::new(new_env)
    }

    /// Handle a pending yield after a VmAwareFn call.
    fn handle_pending_yield_after_call(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        ip: usize,
        yielded_value: Value,
    ) -> Result<Option<VmResult>, String> {
        let coroutine = match self.current_coroutine() {
            Some(co) => co.clone(),
            None => {
                return Err("pending yield outside of coroutine".to_string());
            }
        };

        let saved_stack: Vec<Value> = self.stack.drain(..).collect();

        let frame = crate::value::ContinuationFrame {
            bytecode: Rc::new(bytecode.to_vec()),
            constants: Rc::new(constants.to_vec()),
            env: closure_env.cloned().unwrap_or_else(|| Rc::new(vec![])),
            ip,
            stack: saved_stack,
            exception_handlers: self.exception_handlers.clone(),
            handling_exception: self.handling_exception,
        };

        let cont_data = crate::value::ContinuationData::new(frame);
        let continuation = Value::continuation(cont_data);

        {
            let mut co = coroutine.borrow_mut();
            co.state = CoroutineState::Suspended;
            co.yielded_value = Some(yielded_value);
        }

        Ok(Some(VmResult::Yielded {
            value: yielded_value,
            continuation,
        }))
    }
}
