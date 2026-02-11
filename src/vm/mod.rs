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

pub use core::{is_exception_subclass, CallFrame, VM};

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)
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
                return Ok(result);
            }
        }
    }

    /// Inner execution loop that handles all instructions except tail calls
    fn execute_bytecode_inner(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        let mut ip = 0;
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
                    return control::handle_return(self);
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

                    let result = match func {
                        Value::NativeFn(f) => {
                            match f(&args) {
                                Ok(val) => val,
                                Err(msg) if msg == "Division by zero" => {
                                    // Create a division-by-zero exception
                                    let mut cond = crate::value::Condition::new(4);
                                    // Try to set dividend and divisor if we have the arguments
                                    if args.len() >= 2 {
                                        cond.set_field(0, args[0].clone()); // dividend
                                        cond.set_field(1, args[1].clone()); // divisor
                                    }
                                    self.current_exception = Some(std::rc::Rc::new(cond));
                                    // Push Nil to keep stack consistent
                                    Value::Nil
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        Value::Closure(closure) => {
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
                            match closure.arity {
                                crate::value::Arity::Exact(n) => {
                                    if args.len() != n {
                                        return Err(format!(
                                            "Function expects {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::AtLeast(n) => {
                                    if args.len() < n {
                                        return Err(format!(
                                            "Function expects at least {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::Range(min, max) => {
                                    if args.len() < min || args.len() > max {
                                        return Err(format!(
                                            "Function expects {}-{} arguments, got {}",
                                            min,
                                            max,
                                            args.len()
                                        ));
                                    }
                                }
                            }

                            // Create a new environment that includes:
                            // [captured_vars..., parameters..., locally_defined_cells...]
                            // The closure's env contains captured variables
                            // We append the arguments as parameters
                            // We append empty cells for locally-defined variables (Phase 4)
                            let mut new_env = Vec::new();
                            new_env.extend((*closure.env).iter().cloned());
                            new_env.extend(args.clone());

                            // Calculate number of locally-defined variables
                            let num_params = match closure.arity {
                                crate::value::Arity::Exact(n) => n,
                                crate::value::Arity::AtLeast(n) => n,
                                crate::value::Arity::Range(min, _) => min,
                            };
                            let num_locally_defined = closure
                                .num_locals
                                .saturating_sub(num_params + closure.num_captures);

                            // Add empty cells for locally-defined variables
                            // These will be initialized when define statements execute
                            for _ in 0..num_locally_defined {
                                let empty_cell = Value::Cell(std::rc::Rc::new(
                                    std::cell::RefCell::new(Box::new(Value::Nil)),
                                ));
                                new_env.push(empty_cell);
                            }

                            let new_env_rc = std::rc::Rc::new(new_env);

                            let result = self.execute_bytecode(
                                &closure.bytecode,
                                &closure.constants,
                                Some(&new_env_rc),
                            )?;

                            self.call_depth -= 1;
                            result
                        }
                        Value::JitClosure(jit_closure) => {
                            // Validate argument count
                            match jit_closure.arity {
                                crate::value::Arity::Exact(n) => {
                                    if args.len() != n {
                                        return Err(format!(
                                            "JIT closure expects {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::AtLeast(n) => {
                                    if args.len() < n {
                                        return Err(format!(
                                            "JIT closure expects at least {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::Range(min, max) => {
                                    if args.len() < min || args.len() > max {
                                        return Err(format!(
                                            "JIT closure expects {}-{} arguments, got {}",
                                            min,
                                            max,
                                            args.len()
                                        ));
                                    }
                                }
                            }

                            // Check if we have real native code
                            if !jit_closure.code_ptr.is_null() {
                                // Call native code!
                                unsafe {
                                    // Prepare args array
                                    let args_encoded: Vec<i64> =
                                        args.iter().map(encode_value_for_jit).collect();

                                    // Prepare env array (captures)
                                    let env_encoded: Vec<i64> =
                                        jit_closure.env.iter().map(encode_value_for_jit).collect();

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
                                }
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
                                new_env.extend(args.clone());

                                // Calculate number of locally-defined variables
                                let num_params = match source.arity {
                                    crate::value::Arity::Exact(n) => n,
                                    crate::value::Arity::AtLeast(n) => n,
                                    crate::value::Arity::Range(min, _) => min,
                                };
                                let num_locally_defined = source
                                    .num_locals
                                    .saturating_sub(num_params + source.num_captures);

                                // Add empty cells for locally-defined variables
                                for _ in 0..num_locally_defined {
                                    let empty_cell = Value::Cell(std::rc::Rc::new(
                                        std::cell::RefCell::new(Box::new(Value::Nil)),
                                    ));
                                    new_env.push(empty_cell);
                                }

                                let new_env_rc = std::rc::Rc::new(new_env);

                                let result = self.execute_bytecode(
                                    &source.bytecode,
                                    &source.constants,
                                    Some(&new_env_rc),
                                )?;

                                self.call_depth -= 1;
                                result
                            } else {
                                return Err("JIT closure has no fallback source".to_string());
                            }
                        }
                        _ => return Err(format!("Cannot call {:?}", func)),
                    };

                    self.stack.push(result);
                }

                Instruction::TailCall => {
                    let arg_count = self.read_u8(bytecode, &mut ip) as usize;
                    let func = self.stack.pop().ok_or("Stack underflow")?;

                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().ok_or("Stack underflow")?);
                    }
                    args.reverse();

                    match func {
                        Value::NativeFn(f) => {
                            return f(&args);
                        }
                        Value::Closure(closure) => {
                            // Build proper environment: captures + args + locals (same as Call)
                            // Reuse the cached environment vector to avoid repeated allocations
                            self.tail_call_env_cache.clear();
                            self.tail_call_env_cache
                                .extend((*closure.env).iter().cloned());
                            self.tail_call_env_cache.extend(args);

                            // Calculate number of locally-defined variables
                            let num_params = match closure.arity {
                                crate::value::Arity::Exact(n) => n,
                                crate::value::Arity::AtLeast(n) => n,
                                crate::value::Arity::Range(min, _) => min,
                            };
                            let num_locally_defined = closure
                                .num_locals
                                .saturating_sub(num_params + closure.num_captures);

                            // Add empty cells for locally-defined variables
                            for _ in 0..num_locally_defined {
                                let empty_cell = Value::Cell(std::rc::Rc::new(
                                    std::cell::RefCell::new(Box::new(Value::Nil)),
                                ));
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
                            return Ok(Value::Nil);
                        }
                        Value::JitClosure(jit_closure) => {
                            // Validate argument count
                            match jit_closure.arity {
                                crate::value::Arity::Exact(n) => {
                                    if args.len() != n {
                                        return Err(format!(
                                            "JIT closure expects {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::AtLeast(n) => {
                                    if args.len() < n {
                                        return Err(format!(
                                            "JIT closure expects at least {} arguments, got {}",
                                            n,
                                            args.len()
                                        ));
                                    }
                                }
                                crate::value::Arity::Range(min, max) => {
                                    if args.len() < min || args.len() > max {
                                        return Err(format!(
                                            "JIT closure expects {}-{} arguments, got {}",
                                            min,
                                            max,
                                            args.len()
                                        ));
                                    }
                                }
                            }

                            // Check if we have real native code
                            if !jit_closure.code_ptr.is_null() {
                                // Call native code directly (tail call optimization)
                                return unsafe {
                                    // Prepare args array
                                    let args_encoded: Vec<i64> =
                                        args.iter().map(encode_value_for_jit).collect();

                                    // Prepare env array (captures)
                                    let env_encoded: Vec<i64> =
                                        jit_closure.env.iter().map(encode_value_for_jit).collect();

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
                                    decode_jit_result(result_encoded)
                                };
                            } else if let Some(ref source) = jit_closure.source {
                                // Build proper environment: captures + args + locals (same as Call)
                                self.tail_call_env_cache.clear();
                                self.tail_call_env_cache
                                    .extend((*source.env).iter().cloned());
                                self.tail_call_env_cache.extend(args);

                                // Calculate number of locally-defined variables
                                let num_params = match source.arity {
                                    crate::value::Arity::Exact(n) => n,
                                    crate::value::Arity::AtLeast(n) => n,
                                    crate::value::Arity::Range(min, _) => min,
                                };
                                let num_locally_defined = source
                                    .num_locals
                                    .saturating_sub(num_params + source.num_captures);

                                // Add empty cells for locally-defined variables
                                for _ in 0..num_locally_defined {
                                    let empty_cell = Value::Cell(std::rc::Rc::new(
                                        std::cell::RefCell::new(Box::new(Value::Nil)),
                                    ));
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
                                return Ok(Value::Nil);
                            } else {
                                return Err("JIT closure has no fallback source".to_string());
                            }
                        }
                        _ => return Err(format!("Cannot call {:?}", func)),
                    };
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
                    let handler_offset = self.read_i16(bytecode, &mut ip);
                    let finally_offset_val = self.read_i16(bytecode, &mut ip);
                    let finally_offset = if finally_offset_val == -1 {
                        None
                    } else {
                        Some(finally_offset_val)
                    };

                    // Push handler frame to exception_handlers stack
                    use crate::vm::core::ExceptionHandler;
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
                    self.stack.push(Value::Bool(matches));
                }

                Instruction::BindException => {
                    // Bind caught exception to a variable
                    // Read constant index that contains the symbol
                    let const_idx = self.read_u16(bytecode, &mut ip) as usize;

                    // Get the current exception if it exists
                    if let Some(exc) = &self.current_exception {
                        // Extract the symbol ID from constants
                        if let Some(Value::Symbol(sym_id)) = constants.get(const_idx) {
                            // Bind the exception to the variable in the current scope
                            // For now, use globals as a simple binding mechanism
                            self.globals.insert(sym_id.0, Value::Condition(exc.clone()));
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

                Instruction::InvokeRestart => {
                    // TODO: Implement invoke restart
                    // Invoke a restart by name
                    let _restart_name_id = self.read_u16(bytecode, &mut ip);
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
                    // No handler for this exception - propagate as error
                    if let Some(exc) = &self.current_exception {
                        return Err(format!("Unhandled exception: {}", exc.exception_id));
                    }
                    return Err("Unhandled exception".to_string());
                }
            }
        }
    }
}

/// Encode a Value for passing to JIT code
fn encode_value_for_jit(value: &Value) -> i64 {
    match value {
        Value::Nil => 0,
        Value::Bool(false) => 0,
        Value::Bool(true) => 1,
        Value::Int(n) => *n,
        // For heap values, we pass the pointer
        // IMPORTANT: The value must be kept alive during JIT execution!
        other => {
            // This is unsafe - we're passing a raw pointer
            // The caller must ensure the Value lives long enough
            other as *const Value as i64
        }
    }
}

/// Decode a result from JIT code back to Value
fn decode_jit_result(encoded: i64) -> Result<Value, String> {
    // For now, assume integer result
    // TODO: Need type information to properly decode
    Ok(Value::Int(encoded))
}
