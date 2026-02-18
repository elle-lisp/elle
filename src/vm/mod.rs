pub mod arithmetic;
pub mod closure;
pub mod comparison;
pub mod control;
pub mod core;
pub mod data;
pub mod literals;
pub mod scope;
pub mod stack;
pub mod types;
pub mod variables;

pub use crate::value::condition::{exception_parent, is_exception_subclass};
pub use core::{CallFrame, VmResult, VM};

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use crate::value_old::CoroutineState;
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        let result = self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)?;
        // Check if an exception escaped all handlers at the top level
        if let Some(exc) = &self.current_exception {
            // Use the exception's message for the error
            Err(format!("{}", exc))
        } else {
            Ok(result)
        }
    }

    /// Check arity and set exception if mismatch
    /// Returns true if arity is OK, false if there's a mismatch (exception is set)
    fn check_arity(&mut self, arity: &crate::value_old::Arity, arg_count: usize) -> bool {
        match arity {
            crate::value_old::Arity::Exact(n) => {
                if arg_count != *n {
                    let msg = format!("expected {} arguments, got {}", n, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*n as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
            crate::value_old::Arity::AtLeast(n) => {
                if arg_count < *n {
                    let msg = format!("expected at least {} arguments, got {}", n, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*n as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
            crate::value_old::Arity::Range(min, max) => {
                if arg_count < *min || arg_count > *max {
                    let msg = format!("expected {}-{} arguments, got {}", min, max, arg_count);
                    let mut cond = crate::value::Condition::arity_error(msg)
                        .with_field(0, Value::int(*min as i64))
                        .with_field(1, Value::int(arg_count as i64));
                    if let Some(loc) = self.current_source_loc.clone() {
                        cond.location = Some(loc);
                    }
                    self.current_exception = Some(std::rc::Rc::new(cond));
                    return false;
                }
            }
        }
        true
    }

    /// Execute raw bytecode with optional closure environment
    ///
    /// This is used internally for closure execution and by spawn/join primitives
    /// to execute closures in spawned threads.
    ///
    /// This function handles tail calls without recursion by using a loop-based approach.
    /// When a tail call is encountered, instead of recursively calling execute_bytecode,
    /// we store the tail call information and loop back to execute it.
    pub fn execute_bytecode(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        // Outer loop to handle tail calls without recursion
        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();

        loop {
            let result = self.execute_bytecode_inner(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
            )?;

            // Check if there's a pending tail call
            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
                // Continue the loop to execute the tail call
            } else {
                // No pending tail call, return the result
                return match result {
                    VmResult::Done(v) => Ok(v),
                    VmResult::Yielded { .. } => {
                        // Yield should be handled by coroutine_resume, not here
                        Err("Unexpected yield outside coroutine context".to_string())
                    }
                };
            }
        }
    }

    /// Inner execution loop that handles all instructions except tail calls
    /// This is the implementation; the public wrapper handles handler isolation.
    fn execute_bytecode_inner_impl(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        let mut ip = start_ip;
        let mut instruction_count = 0;
        const MAX_INSTRUCTIONS: usize = 100000; // Safety limit to prevent infinite loops

        loop {
            instruction_count += 1;
            if instruction_count > MAX_INSTRUCTIONS {
                let instr_byte = if ip < bytecode.len() {
                    bytecode[ip]
                } else {
                    255
                };
                return Err(format!(
                    "Instruction limit exceeded at ip={} (instr={}), stack depth={}, exception={}",
                    ip,
                    instr_byte,
                    self.stack.len(),
                    self.current_exception.is_some()
                ));
            }

            // Check for pending exception at the START of each iteration.
            // This is important for continuation resume: if an inner frame returned
            // with an exception, we need to handle it before executing any instructions
            // in this frame. Without this check, we'd execute the next instruction
            // (e.g., PopHandler) before noticing the exception.
            if self.current_exception.is_some() && !self.handling_exception {
                if let Some(handler) = self.exception_handlers.last() {
                    // Unwind stack to saved depth
                    while self.stack.len() > handler.stack_depth {
                        self.stack.pop();
                    }
                    // Mark that we're handling an exception
                    self.handling_exception = true;
                    // Jump to handler code (handler_offset is absolute bytecode position)
                    ip = handler.handler_offset as usize;
                    continue; // Skip to next iteration to execute handler code
                } else {
                    // No local handler — return normally, leaving current_exception set.
                    return Ok(VmResult::Done(Value::NIL));
                }
            }

            if ip >= bytecode.len() {
                return Err("Unexpected end of bytecode".to_string());
            }

            let instr_byte = bytecode[ip];
            ip += 1;

            let instr: Instruction = unsafe { std::mem::transmute(instr_byte) };

            match instr {
                // Stack operations
                Instruction::LoadConst => {
                    stack::handle_load_const(self, bytecode, &mut ip, constants);
                }

                Instruction::LoadLocal => {
                    stack::handle_load_local(self, bytecode, &mut ip)?;
                }

                Instruction::Pop => {
                    stack::handle_pop(self)?;
                }

                Instruction::Dup => {
                    stack::handle_dup(self)?;
                }

                Instruction::DupN => {
                    stack::handle_dup_n(self, bytecode, &mut ip)?;
                }

                // Variable access
                Instruction::LoadGlobal => {
                    variables::handle_load_global(self, bytecode, &mut ip, constants)?;
                }

                Instruction::StoreGlobal => {
                    variables::handle_store_global(self, bytecode, &mut ip, constants)?;
                }

                Instruction::StoreLocal => {
                    variables::handle_store_local(self, bytecode, &mut ip)?;
                }

                Instruction::LoadUpvalue => {
                    variables::handle_load_upvalue(self, bytecode, &mut ip, closure_env)?;
                }

                Instruction::LoadUpvalueRaw => {
                    variables::handle_load_upvalue_raw(self, bytecode, &mut ip, closure_env)?;
                }

                Instruction::StoreUpvalue => {
                    variables::handle_store_upvalue(self, bytecode, &mut ip, closure_env)?;
                }

                // Control flow
                Instruction::Jump => {
                    control::handle_jump(bytecode, &mut ip, self);
                }

                Instruction::JumpIfFalse => {
                    control::handle_jump_if_false(bytecode, &mut ip, self)?;
                }

                Instruction::JumpIfTrue => {
                    control::handle_jump_if_true(bytecode, &mut ip, self)?;
                }

                Instruction::Return => {
                    let value = control::handle_return(self)?;
                    return Ok(VmResult::Done(value));
                }

                // Call instructions (complex, handled inline)
                Instruction::Call => {
                    let arg_count = self.read_u8(bytecode, &mut ip) as usize;
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
                    } else if let Some(f) = func.as_vm_aware_fn() {
                        let result = f(args.as_slice(), self)?;
                        self.stack.push(result);
                    } else if let Some(closure) = func.as_closure() {
                        self.call_depth += 1;
                        if self.call_depth > 1000 {
                            return Err("Stack overflow".to_string());
                        }

                        // Record this closure call for profiling (only track hot closures)
                        let bytecode_ptr = closure.bytecode.as_ptr();
                        let is_hot = self.record_closure_call(bytecode_ptr);
                        if is_hot {
                            // TODO: When hot, attempt to compile and cache this closure
                            // For now, just track that it's hot
                        }

                        // Validate argument count
                        if !self.check_arity(&closure.arity, args.len()) {
                            self.call_depth -= 1;
                            self.stack.push(Value::NIL);
                        } else {
                            // Create a new environment that includes:
                            // [captured_vars..., parameters..., locally_defined_cells...]
                            // The closure's env contains captured variables
                            // We append the arguments as parameters
                            // We append empty cells for locally-defined variables (Phase 4)
                            let mut new_env = Vec::new();
                            new_env.extend((*closure.env).iter().cloned());

                            // Add parameters, wrapping in local cells if cell_params_mask indicates
                            for (i, arg) in args.iter().enumerate() {
                                if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                                    // This parameter is mutated - wrap it in a local cell
                                    new_env.push(Value::local_cell(*arg));
                                } else {
                                    new_env.push(*arg);
                                }
                            }

                            // Calculate number of locally-defined variables
                            let num_params = match closure.arity {
                                crate::value_old::Arity::Exact(n) => n,
                                crate::value_old::Arity::AtLeast(n) => n,
                                crate::value_old::Arity::Range(min, _) => min,
                            };
                            let num_locally_defined = closure.num_locals.saturating_sub(num_params);

                            // Add empty LocalCells for locally-defined variables
                            // These will be initialized when define statements execute
                            // LocalCell is auto-unwrapped by LoadUpvalue (unlike user Cell)
                            for _ in 0..num_locally_defined {
                                let empty_cell = Value::local_cell(Value::NIL);
                                new_env.push(empty_cell);
                            }

                            let new_env_rc = std::rc::Rc::new(new_env);

                            // If we're in a coroutine context, use coroutine-aware execution
                            // that can handle yields from called functions
                            if self.in_coroutine() {
                                let result = self.execute_bytecode_coroutine(
                                    &closure.bytecode,
                                    &closure.constants,
                                    Some(&new_env_rc),
                                )?;

                                self.call_depth -= 1;

                                // If the called function yielded, propagate it up
                                // with the caller's frame prepended to the continuation
                                match result {
                                    VmResult::Done(v) => {
                                        self.stack.push(v);
                                    }
                                    VmResult::Yielded {
                                        value,
                                        continuation,
                                    } => {
                                        // Capture the caller's frame and append it to the continuation.
                                        // self.stack has been restored by execute_bytecode_coroutine
                                        // to the caller's stack state (stuff before the Call args).
                                        let caller_stack: Vec<Value> =
                                            self.stack.drain(..).collect();

                                        let caller_frame = crate::value::ContinuationFrame {
                                            bytecode: Rc::new(bytecode.to_vec()),
                                            constants: Rc::new(constants.to_vec()),
                                            env: closure_env
                                                .cloned()
                                                .unwrap_or_else(|| Rc::new(vec![])),
                                            ip, // IP is right after the Call instruction
                                            stack: caller_stack,
                                            // Save caller's exception handler state
                                            exception_handlers: self.exception_handlers.clone(),
                                            handling_exception: self.handling_exception,
                                        };

                                        // Clone the continuation data and append caller's frame
                                        let mut cont_data = continuation
                                            .as_continuation()
                                            .expect(
                                                "Yielded continuation must be a continuation value",
                                            )
                                            .as_ref()
                                            .clone();
                                        cont_data.append_frame(caller_frame);

                                        let new_continuation = Value::continuation(cont_data);
                                        return Ok(VmResult::Yielded {
                                            value,
                                            continuation: new_continuation,
                                        });
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
                        }
                    } else if let Some(jit_closure) = func.as_jit_closure() {
                        // Validate argument count
                        if !self.check_arity(&jit_closure.arity, args.len()) {
                            self.stack.push(Value::NIL);
                        } else if !jit_closure.code_ptr.is_null() {
                            // Call native code!
                            let result = unsafe {
                                // Prepare args array
                                let args_encoded: Vec<i64> =
                                    args.iter().map(encode_value_for_jit).collect();

                                // Prepare env array (captures) - uses old Value type
                                let env_encoded: Vec<i64> = jit_closure
                                    .env
                                    .iter()
                                    .map(encode_old_value_for_jit)
                                    .collect();

                                // Cast to function pointer
                                // Signature: fn(args_ptr: i64, args_len: i64, env_ptr: i64) -> i64
                                let func: extern "C" fn(i64, i64, i64) -> i64 =
                                    std::mem::transmute(jit_closure.code_ptr);

                                // Call it
                                let result_encoded = func(
                                    args_encoded.as_ptr() as i64,
                                    args_encoded.len() as i64,
                                    env_encoded.as_ptr() as i64,
                                );

                                // Decode result
                                decode_jit_result(result_encoded)?
                            };
                            self.stack.push(result);
                        } else if let Some(ref source) = jit_closure.source {
                            // Fall back to interpreted execution of the source closure
                            self.call_depth += 1;
                            if self.call_depth > 1000 {
                                return Err("Stack overflow".to_string());
                            }

                            // Create a new environment that includes:
                            // [captured_vars..., parameters..., locally_defined_cells...]
                            let mut new_env = Vec::new();
                            new_env.extend((*source.env).iter().cloned());

                            // Add parameters, wrapping in local cells if cell_params_mask indicates
                            for (i, arg) in args.iter().enumerate() {
                                if i < 64 && (source.cell_params_mask & (1 << i)) != 0 {
                                    // This parameter is mutated - wrap it in a local cell
                                    new_env.push(Value::local_cell(*arg));
                                } else {
                                    new_env.push(*arg);
                                }
                            }

                            // Calculate number of locally-defined variables
                            let num_params = match source.arity {
                                crate::value_old::Arity::Exact(n) => n,
                                crate::value_old::Arity::AtLeast(n) => n,
                                crate::value_old::Arity::Range(min, _) => min,
                            };
                            let num_locally_defined = source.num_locals.saturating_sub(num_params);

                            // Add empty LocalCells for locally-defined variables
                            for _ in 0..num_locally_defined {
                                let empty_cell = Value::local_cell(Value::NIL);
                                new_env.push(empty_cell);
                            }

                            let new_env_rc = std::rc::Rc::new(new_env);

                            // If we're in a coroutine context, use coroutine-aware execution
                            if self.in_coroutine() {
                                let result = self.execute_bytecode_coroutine(
                                    &source.bytecode,
                                    &source.constants,
                                    Some(&new_env_rc),
                                )?;

                                self.call_depth -= 1;

                                // If the called function yielded, propagate it up
                                // with the caller's frame prepended to the continuation
                                match result {
                                    VmResult::Done(v) => {
                                        self.stack.push(v);
                                    }
                                    VmResult::Yielded {
                                        value,
                                        continuation,
                                    } => {
                                        // Capture the caller's frame and append it to the continuation.
                                        let caller_stack: Vec<Value> =
                                            self.stack.drain(..).collect();

                                        let caller_frame = crate::value::ContinuationFrame {
                                            bytecode: Rc::new(bytecode.to_vec()),
                                            constants: Rc::new(constants.to_vec()),
                                            env: closure_env
                                                .cloned()
                                                .unwrap_or_else(|| Rc::new(vec![])),
                                            ip,
                                            stack: caller_stack,
                                            // Save caller's exception handler state
                                            exception_handlers: self.exception_handlers.clone(),
                                            handling_exception: self.handling_exception,
                                        };

                                        let mut cont_data = continuation
                                            .as_continuation()
                                            .expect(
                                                "Yielded continuation must be a continuation value",
                                            )
                                            .as_ref()
                                            .clone();
                                        cont_data.append_frame(caller_frame);

                                        let new_continuation = Value::continuation(cont_data);
                                        return Ok(VmResult::Yielded {
                                            value,
                                            continuation: new_continuation,
                                        });
                                    }
                                }
                            } else {
                                let result = self.execute_bytecode(
                                    &source.bytecode,
                                    &source.constants,
                                    Some(&new_env_rc),
                                )?;

                                self.call_depth -= 1;
                                self.stack.push(result);
                            }
                        } else {
                            return Err("JIT closure has no fallback source".to_string());
                        }
                    } else {
                        return Err(format!("Cannot call {:?}", func));
                    }
                }

                Instruction::TailCall => {
                    let arg_count = self.read_u8(bytecode, &mut ip) as usize;
                    let func = self.stack.pop().ok_or("Stack underflow")?;

                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().ok_or("Stack underflow")?);
                    }
                    args.reverse();

                    if let Some(f) = func.as_native_fn() {
                        match f(&args) {
                            Ok(val) => return Ok(VmResult::Done(val)),
                            Err(cond) => {
                                self.current_exception = Some(std::rc::Rc::new(cond));
                                return Ok(VmResult::Done(Value::NIL));
                            }
                        }
                    } else if let Some(f) = func.as_vm_aware_fn() {
                        return f(&args, self)
                            .map(VmResult::Done)
                            .map_err(|e| e.description());
                    } else if let Some(closure) = func.as_closure() {
                        // Validate argument count
                        if !self.check_arity(&closure.arity, args.len()) {
                            // Exception was set, check it before returning
                            if self.current_exception.is_some()
                                && !self.handling_exception
                                && self.exception_handlers.is_empty()
                            {
                                // No local handler — propagate via current_exception
                                return Ok(VmResult::Done(Value::NIL));
                            }
                            return Ok(VmResult::Done(Value::NIL));
                        }

                        // Build proper environment: captures + args + locals (same as Call)
                        // Reuse the cached environment vector to avoid repeated allocations
                        self.tail_call_env_cache.clear();
                        self.tail_call_env_cache
                            .extend((*closure.env).iter().cloned());

                        // Add parameters, wrapping in local cells if cell_params_mask indicates
                        for (i, arg) in args.iter().enumerate() {
                            if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                                // This parameter is mutated - wrap it in a local cell
                                self.tail_call_env_cache.push(Value::local_cell(*arg));
                            } else {
                                self.tail_call_env_cache.push(*arg);
                            }
                        }

                        // Calculate number of locally-defined variables
                        let num_params = match closure.arity {
                            crate::value_old::Arity::Exact(n) => n,
                            crate::value_old::Arity::AtLeast(n) => n,
                            crate::value_old::Arity::Range(min, _) => min,
                        };
                        let num_locally_defined = closure.num_locals.saturating_sub(num_params);

                        // Add empty LocalCells for locally-defined variables
                        for _ in 0..num_locally_defined {
                            let empty_cell = Value::local_cell(Value::NIL);
                            self.tail_call_env_cache.push(empty_cell);
                        }

                        let new_env_rc = std::rc::Rc::new(self.tail_call_env_cache.clone());

                        // Store the tail call information to be executed in the outer loop
                        // instead of recursively calling execute_bytecode
                        // Don't increment call_depth — this is the tail call optimization
                        self.pending_tail_call = Some((
                            (*closure.bytecode).clone(),
                            (*closure.constants).clone(),
                            new_env_rc,
                        ));

                        // Return a dummy value - the outer loop will detect the pending tail call
                        // and execute it instead of returning this value
                        return Ok(VmResult::Done(Value::NIL));
                    } else if let Some(jit_closure) = func.as_jit_closure() {
                        // Validate argument count
                        if !self.check_arity(&jit_closure.arity, args.len()) {
                            // Exception was set, check it before returning
                            if self.current_exception.is_some()
                                && !self.handling_exception
                                && self.exception_handlers.is_empty()
                            {
                                // No local handler — propagate via current_exception
                                return Ok(VmResult::Done(Value::NIL));
                            }
                            return Ok(VmResult::Done(Value::NIL));
                        } else if !jit_closure.code_ptr.is_null() {
                            // Call native code directly (tail call optimization)
                            return unsafe {
                                // Prepare args array
                                let args_encoded: Vec<i64> =
                                    args.iter().map(encode_value_for_jit).collect();

                                // Prepare env array (captures) - uses old Value type
                                let env_encoded: Vec<i64> = jit_closure
                                    .env
                                    .iter()
                                    .map(encode_old_value_for_jit)
                                    .collect();

                                // Cast to function pointer
                                let func: extern "C" fn(i64, i64, i64) -> i64 =
                                    std::mem::transmute(jit_closure.code_ptr);

                                // Call it
                                let result_encoded = func(
                                    args_encoded.as_ptr() as i64,
                                    args_encoded.len() as i64,
                                    env_encoded.as_ptr() as i64,
                                );

                                // Decode result
                                decode_jit_result(result_encoded).map(VmResult::Done)
                            };
                        } else if let Some(ref source) = jit_closure.source {
                            // Build proper environment: captures + args + locals (same as Call)
                            self.tail_call_env_cache.clear();
                            self.tail_call_env_cache
                                .extend((*source.env).iter().cloned());

                            // Add parameters, wrapping in local cells if cell_params_mask indicates
                            for (i, arg) in args.iter().enumerate() {
                                if i < 64 && (source.cell_params_mask & (1 << i)) != 0 {
                                    // This parameter is mutated - wrap it in a local cell
                                    self.tail_call_env_cache.push(Value::local_cell(*arg));
                                } else {
                                    self.tail_call_env_cache.push(*arg);
                                }
                            }

                            // Calculate number of locally-defined variables
                            let num_params = match source.arity {
                                crate::value_old::Arity::Exact(n) => n,
                                crate::value_old::Arity::AtLeast(n) => n,
                                crate::value_old::Arity::Range(min, _) => min,
                            };
                            let num_locally_defined = source.num_locals.saturating_sub(num_params);

                            // Add empty LocalCells for locally-defined variables
                            for _ in 0..num_locally_defined {
                                let empty_cell = Value::local_cell(Value::NIL);
                                self.tail_call_env_cache.push(empty_cell);
                            }

                            let new_env_rc = std::rc::Rc::new(self.tail_call_env_cache.clone());

                            // Store the tail call information to be executed in the outer loop
                            self.pending_tail_call = Some((
                                (*source.bytecode).clone(),
                                (*source.constants).clone(),
                                new_env_rc,
                            ));

                            // Return a dummy value - the outer loop will detect the pending tail call
                            return Ok(VmResult::Done(Value::NIL));
                        } else {
                            return Err("JIT closure has no fallback source".to_string());
                        }
                    } else {
                        return Err(format!("Cannot call {:?}", func));
                    }
                }

                // Closures
                Instruction::MakeClosure => {
                    closure::handle_make_closure(self, bytecode, &mut ip, constants)?;
                }

                // Data structures
                Instruction::Cons => {
                    data::handle_cons(self)?;
                }

                Instruction::Car => {
                    data::handle_car(self)?;
                }

                Instruction::Cdr => {
                    data::handle_cdr(self)?;
                }

                Instruction::MakeVector => {
                    data::handle_make_vector(self, bytecode, &mut ip)?;
                }

                Instruction::VectorRef => {
                    data::handle_vector_ref(self)?;
                }

                Instruction::VectorSet => {
                    data::handle_vector_set(self)?;
                }

                // Arithmetic (integer)
                Instruction::AddInt => {
                    arithmetic::handle_add_int(self)?;
                }

                Instruction::SubInt => {
                    arithmetic::handle_sub_int(self)?;
                }

                Instruction::MulInt => {
                    arithmetic::handle_mul_int(self)?;
                }

                Instruction::DivInt => {
                    arithmetic::handle_div_int(self)?;
                }

                // Arithmetic (polymorphic)
                Instruction::Add => {
                    arithmetic::handle_add(self)?;
                }

                Instruction::Sub => {
                    arithmetic::handle_sub(self)?;
                }

                Instruction::Mul => {
                    arithmetic::handle_mul(self)?;
                }

                Instruction::Div => {
                    arithmetic::handle_div(self)?;
                }

                // Comparisons
                Instruction::Eq => {
                    comparison::handle_eq(self)?;
                }

                Instruction::Lt => {
                    comparison::handle_lt(self)?;
                }

                Instruction::Gt => {
                    comparison::handle_gt(self)?;
                }

                Instruction::Le => {
                    comparison::handle_le(self)?;
                }

                Instruction::Ge => {
                    comparison::handle_ge(self)?;
                }

                // Type checks
                Instruction::IsNil => {
                    types::handle_is_nil(self)?;
                }

                Instruction::IsEmptyList => {
                    types::handle_is_empty_list(self)?;
                }

                Instruction::IsPair => {
                    types::handle_is_pair(self)?;
                }

                Instruction::IsNumber => {
                    types::handle_is_number(self)?;
                }

                Instruction::IsSymbol => {
                    types::handle_is_symbol(self)?;
                }

                Instruction::Not => {
                    types::handle_not(self)?;
                }

                // Literals
                Instruction::Nil => {
                    literals::handle_nil(self);
                }

                Instruction::EmptyList => {
                    literals::handle_empty_list(self);
                }

                Instruction::True => {
                    literals::handle_true(self);
                }

                Instruction::False => {
                    literals::handle_false(self);
                }

                // Scope management (Phase 2)
                Instruction::PushScope => {
                    // Read scope type from bytecode
                    let scope_type_byte = bytecode[ip];
                    ip += 1;
                    scope::handle_push_scope(self, scope_type_byte)?;
                }

                Instruction::PopScope => {
                    scope::handle_pop_scope(self)?;
                }

                Instruction::DefineLocal => {
                    scope::handle_define_local(self, bytecode, &mut ip, constants)?;
                }

                // Cell operations for shared mutable captures (Phase 4)
                Instruction::MakeCell => {
                    scope::handle_make_cell(self)?;
                }

                Instruction::UnwrapCell => {
                    scope::handle_unwrap_cell(self)?;
                }

                Instruction::UpdateCell => {
                    scope::handle_update_cell(self)?;
                }

                // Exception handling (Phase 3)
                Instruction::PushHandler => {
                    // Read handler_offset (i16) and finally_offset (i16)
                    let handler_offset = self.read_u16(bytecode, &mut ip);
                    let finally_offset_val = self.read_i16(bytecode, &mut ip);
                    let finally_offset = if finally_offset_val == -1 {
                        None
                    } else {
                        Some(finally_offset_val)
                    };

                    // Push handler frame to exception_handlers stack
                    use crate::value::ExceptionHandler;
                    self.exception_handlers.push(ExceptionHandler {
                        handler_offset,
                        finally_offset,
                        stack_depth: self.stack.len(),
                    });
                }

                Instruction::PopHandler => {
                    // Pop from exception_handlers stack when handler completes successfully
                    self.exception_handlers.pop();
                }

                Instruction::CreateHandler => {
                    // TODO: Implement create handler
                    // Create handler context
                    let _handler_fn_idx = self.read_u16(bytecode, &mut ip);
                    let _condition_id = self.read_u16(bytecode, &mut ip);
                }

                Instruction::CheckException => {
                    // Verify that an exception has occurred
                    // This instruction should only be reached if an exception occurred
                    // (Normal path jumps past this via the Jump after PopHandler)
                    if self.current_exception.is_none() {
                        // This shouldn't happen, but if it does, we have a bug
                        return Err("CheckException reached with no exception set".to_string());
                    }
                    // Exception is set, fall through to handler matching code
                }

                Instruction::MatchException => {
                    // Read handler exception ID from bytecode as immediate
                    let handler_id = self.read_u16(bytecode, &mut ip);

                    // Check if current exception matches the handler's exception ID
                    // Uses inheritance matching: catching a parent type catches children
                    let matches = if let Some(exc) = &self.current_exception {
                        is_exception_subclass(exc.exception_id, handler_id as u32)
                    } else {
                        false
                    };

                    // Push boolean result onto stack for JumpIfFalse to check
                    self.stack.push(Value::bool(matches));
                }

                Instruction::BindException => {
                    // Bind caught exception to a variable
                    // Read constant index that contains the symbol
                    let const_idx = self.read_u16(bytecode, &mut ip) as usize;

                    // Get the current exception if it exists
                    if let Some(exc) = &self.current_exception {
                        // Extract the symbol ID from constants
                        if let Some(const_val) = constants.get(const_idx) {
                            if let Some(sym_id) = const_val.as_symbol() {
                                // Bind the exception to the variable in the current scope
                                // For now, use globals as a simple binding mechanism
                                use crate::value::heap::{alloc, HeapObject};
                                // Convert new Condition to old Condition
                                let new_cond = (**exc).clone();
                                let mut old_cond =
                                    crate::value_old::Condition::new(new_cond.exception_id);
                                // Store message in old condition's FIELD_MESSAGE
                                old_cond.set_field(
                                    crate::value_old::Condition::FIELD_MESSAGE,
                                    crate::value_old::Value::String(
                                        new_cond.message.clone().into(),
                                    ),
                                );
                                for (field_id, value) in new_cond.fields {
                                    let old_value =
                                        crate::primitives::coroutines::new_value_to_old(value);
                                    old_cond.set_field(field_id, old_value);
                                }
                                if let Some(bt) = new_cond.backtrace {
                                    old_cond.backtrace = Some(bt);
                                }
                                if let Some(loc) = new_cond.location {
                                    old_cond.location = Some(loc);
                                }
                                let exc_value = alloc(HeapObject::Condition(old_cond));
                                self.globals.insert(sym_id, exc_value);
                            } else {
                                return Err(
                                    "BindException: Expected symbol in constants".to_string()
                                );
                            }
                        } else {
                            return Err("BindException: Expected symbol in constants".to_string());
                        }
                    }
                }

                Instruction::ClearException => {
                    // Clear current exception
                    self.current_exception = None;
                    // No longer handling exception
                    self.handling_exception = false;
                }

                Instruction::ReraiseException => {
                    // No handler clause matched — re-raise the exception.
                    // Pop this handler so the interrupt mechanism doesn't loop
                    // back to the same handler, and clear the handling flag so
                    // the interrupt mechanism can fire again for the next handler.
                    self.exception_handlers.pop();
                    self.handling_exception = false;
                    // current_exception remains set — the interrupt check at the
                    // bottom of the loop will re-fire and find the next handler
                    // (or the "no handler" path if none remain).
                }

                Instruction::InvokeRestart => {
                    // TODO: Implement invoke restart
                    // Invoke a restart by name
                    let _restart_name_id = self.read_u16(bytecode, &mut ip);
                }

                Instruction::Yield => {
                    // 1. Pop the value to yield
                    let yielded_value = self.stack.pop().ok_or("Stack underflow on yield")?;

                    // 2. Check we're in a coroutine context
                    let coroutine = match self.current_coroutine() {
                        Some(co) => co.clone(),
                        None => return Err("yield used outside of coroutine".to_string()),
                    };

                    // 3. Save execution context as a continuation frame
                    // The stack is drained into the frame's stack field
                    let saved_stack: Vec<Value> = self.stack.drain(..).collect();

                    // Create the innermost continuation frame (the yielding frame)
                    // Save exception handler state so it can be restored on resume
                    let frame = crate::value::ContinuationFrame {
                        bytecode: Rc::new(bytecode.to_vec()),
                        constants: Rc::new(constants.to_vec()),
                        env: closure_env.cloned().unwrap_or_else(|| Rc::new(vec![])),
                        ip, // IP after the Yield instruction
                        stack: saved_stack,
                        exception_handlers: self.exception_handlers.clone(),
                        handling_exception: self.handling_exception,
                    };

                    let cont_data = crate::value::ContinuationData::new(frame);
                    let continuation = Value::continuation(cont_data);

                    // 4. Update coroutine state
                    {
                        let mut co = coroutine.borrow_mut();
                        co.state = CoroutineState::Suspended;
                        co.yielded_value = None;
                    }

                    // 5. Return the yielded value with its continuation
                    return Ok(VmResult::Yielded {
                        value: yielded_value,
                        continuation,
                    });
                }

                Instruction::LoadException => {
                    // Push the current exception value onto the stack
                    // If there's an exception, push it as a Value; otherwise push NIL
                    let exc_value = if let Some(exc) = &self.current_exception {
                        use crate::value::heap::{alloc, HeapObject};
                        // Convert new Condition to old Condition
                        let new_cond = (**exc).clone();
                        let mut old_cond = crate::value_old::Condition::new(new_cond.exception_id);
                        // Store message in old condition's FIELD_MESSAGE
                        old_cond.set_field(
                            crate::value_old::Condition::FIELD_MESSAGE,
                            crate::value_old::Value::String(new_cond.message.clone().into()),
                        );
                        for (field_id, value) in new_cond.fields {
                            let old_value = crate::primitives::coroutines::new_value_to_old(value);
                            old_cond.set_field(field_id, old_value);
                        }
                        if let Some(bt) = new_cond.backtrace {
                            old_cond.backtrace = Some(bt);
                        }
                        if let Some(loc) = new_cond.location {
                            old_cond.location = Some(loc);
                        }
                        alloc(HeapObject::Condition(old_cond))
                    } else {
                        Value::NIL
                    };
                    self.stack.push(exc_value);
                }
            }

            // Phase 9a: Exception interrupt mechanism
            // Check if an exception occurred during instruction execution
            // If yes, jump to the handler if one exists, otherwise propagate error
            // But only jump if we're not already in exception handler code
            if self.current_exception.is_some() && !self.handling_exception {
                if let Some(handler) = self.exception_handlers.last() {
                    // Unwind stack to saved depth
                    while self.stack.len() > handler.stack_depth {
                        self.stack.pop();
                    }
                    // Mark that we're handling an exception
                    self.handling_exception = true;
                    // Jump to handler code (handler_offset is absolute bytecode position)
                    ip = handler.handler_offset as usize;
                } else {
                    // No local handler — return normally, leaving current_exception set.
                    // The caller's interrupt mechanism will handle it after handler
                    // isolation restores the outer frame's handlers.
                    return Ok(VmResult::Done(Value::NIL));
                }
            }
        }
    }

    /// Wrapper that calls execute_bytecode_inner_impl with start_ip = 0
    fn execute_bytecode_inner(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<VmResult, String> {
        self.execute_bytecode_inner_with_ip(bytecode, constants, closure_env, 0)
    }

    /// Execute bytecode starting from a specific instruction pointer.
    /// This is the main entry point for bytecode execution.
    /// Handler isolation happens at the Call instruction level, not here.
    fn execute_bytecode_inner_with_ip(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        // Each bytecode frame has its own exception handler scope.
        // PushHandler/PopHandler are bytecode-local; the inner frame must not
        // see (or jump to) the outer frame's handlers.
        let saved_handlers = std::mem::take(&mut self.exception_handlers);
        let saved_handling = self.handling_exception;
        self.handling_exception = false;

        let result = self.execute_bytecode_inner_impl(bytecode, constants, closure_env, start_ip);

        self.exception_handlers = saved_handlers;
        self.handling_exception = saved_handling;

        result
    }

    /// Execute bytecode starting from a specific instruction pointer
    /// Used for resuming coroutines from where they yielded
    pub fn execute_bytecode_from_ip(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        // This goes through the wrapper which handles handler isolation
        self.execute_bytecode_inner_with_ip(bytecode, constants, closure_env, start_ip)
    }

    /// Execute bytecode starting from a specific IP with pre-set exception handler state.
    ///
    /// This is used when resuming a continuation frame that had active exception handlers
    /// when it was captured. Unlike `execute_bytecode_from_ip`, this method:
    /// 1. Sets the exception handlers to the provided state before execution
    /// 2. Restores the outer handlers after execution
    ///
    /// This ensures that `handler-case` blocks active at yield time remain active
    /// after resume.
    pub fn execute_bytecode_from_ip_with_state(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
        handlers: Vec<crate::value::ExceptionHandler>,
        handling: bool,
    ) -> Result<VmResult, String> {
        // Save outer state
        let saved_handlers = std::mem::replace(&mut self.exception_handlers, handlers);
        let saved_handling = std::mem::replace(&mut self.handling_exception, handling);

        // Execute with tail call loop (similar to execute_bytecode_coroutine)
        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();
        let mut current_ip = start_ip;

        let result = loop {
            let result = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
                current_ip,
            )?;

            // Check for pending tail call
            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
                current_ip = 0; // Tail calls start from the beginning
            } else {
                break result;
            }
        };

        // Restore outer state
        self.exception_handlers = saved_handlers;
        self.handling_exception = saved_handling;

        Ok(result)
    }

    /// Execute bytecode returning VmResult (for coroutine execution)
    pub fn execute_bytecode_coroutine(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<VmResult, String> {
        // Save the caller's stack — the Yield handler will drain self.stack
        // to capture the coroutine's operand state, so we must isolate it.
        let saved_stack = std::mem::take(&mut self.stack);

        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();

        let result = loop {
            let result = self.execute_bytecode_inner(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
            )?;

            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
            } else {
                // If there's an unhandled exception from the coroutine body,
                // leave it on current_exception. The caller (prim_coroutine_resume)
                // will check it and route through the proper exception channel.
                // Do NOT convert to Err(String) — that's the VM bug channel,
                // uncatchable by handler-case.
                break result;
            }
        };

        // Restore the caller's stack
        self.stack = saved_stack;

        Ok(result)
    }
}

/// Encode a Value for passing to JIT code
fn encode_value_for_jit(value: &Value) -> i64 {
    if value.is_nil() {
        0
    } else if let Some(b) = value.as_bool() {
        if b {
            1
        } else {
            0
        }
    } else if let Some(n) = value.as_int() {
        n
    } else {
        // For heap values, we pass the pointer
        // IMPORTANT: The value must be kept alive during JIT execution!
        // This is unsafe - we're passing a raw pointer
        // The caller must ensure the Value lives long enough
        value.to_bits() as i64
    }
}

/// Encode an old Value for passing to JIT code
fn encode_old_value_for_jit(value: &crate::value_old::Value) -> i64 {
    use crate::value_old::Value as OldValue;
    match value {
        OldValue::Nil => 0,
        OldValue::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        OldValue::Int(n) => *n,
        OldValue::Float(f) => f.to_bits() as i64,
        _ => {
            // For other types, we'd need proper encoding
            // For now, just return 0 as a placeholder
            0
        }
    }
}

/// Decode a result from JIT code back to Value
fn decode_jit_result(encoded: i64) -> Result<Value, String> {
    // For now, assume integer result
    // TODO: Need type information to properly decode
    Ok(Value::int(encoded))
}
