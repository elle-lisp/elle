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

pub use core::{CallFrame, VM};

use crate::compiler::bytecode::{Bytecode, Instruction};
use crate::value::Value;
use std::rc::Rc;

impl VM {
    pub fn execute(&mut self, bytecode: &Bytecode) -> Result<Value, String> {
        self.execute_bytecode(&bytecode.instructions, &bytecode.constants, None)
    }

    fn execute_bytecode(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<Value, String> {
        let mut ip = 0;

        loop {
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
                        Value::NativeFn(f) => f(&args)?,
                        Value::Closure(closure) => {
                            self.call_depth += 1;
                            if self.call_depth > 1000 {
                                return Err("Stack overflow".to_string());
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

                            // Create a new environment that includes both captured variables and parameters
                            // The closure's env contains captured variables, and we append the arguments as parameters
                            let mut new_env = Vec::new();
                            new_env.extend((*closure.env).iter().cloned());
                            new_env.extend(args.clone());
                            let new_env_rc = std::rc::Rc::new(new_env);

                            let result = self.execute_bytecode(
                                &closure.bytecode,
                                &closure.constants,
                                Some(&new_env_rc),
                            )?;

                            self.call_depth -= 1;
                            result
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
                            // Build proper environment: captures + args (same as Call)
                            let mut new_env = Vec::new();
                            new_env.extend((*closure.env).iter().cloned());
                            new_env.extend(args);
                            let new_env_rc = std::rc::Rc::new(new_env);

                            // Use closure's own constants table (not parent's)
                            // Don't increment call_depth — this is the tail call optimization
                            return self.execute_bytecode(
                                &closure.bytecode,
                                &closure.constants,
                                Some(&new_env_rc),
                            );
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

                Instruction::LoadScoped => {
                    scope::handle_load_scoped(self, bytecode, &mut ip)?;
                }

                Instruction::StoreScoped => {
                    scope::handle_store_scoped(self, bytecode, &mut ip)?;
                }

                Instruction::DefineLocal => {
                    scope::handle_define_local(self, bytecode, &mut ip, constants)?;
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
                    // Check if an exception has occurred
                    // If current_exception is set, unwind stack and jump to handler code
                    if let Some(handler) = self.exception_handlers.last() {
                        if self.current_exception.is_some() {
                            // Exception occurred - unwind stack to saved depth
                            while self.stack.len() > handler.stack_depth {
                                self.stack.pop();
                            }
                            // Jump to handler code
                            ip = (ip as i32 + handler.handler_offset as i32) as usize;
                        }
                    }
                }

                Instruction::MatchException => {
                    // Read handler exception ID from bytecode as immediate
                    let handler_id = self.read_u16(bytecode, &mut ip);

                    // Check if current exception matches the handler's exception ID
                    let matches = if let Some(exc) = &self.current_exception {
                        exc.exception_id == handler_id as u32
                    } else {
                        false
                    };

                    // Push boolean result onto stack for JumpIfFalse to check
                    self.stack.push(Value::Bool(matches));
                }

                Instruction::BindException => {
                    // Bind caught exception to a variable
                    let var_id = self.read_u16(bytecode, &mut ip);

                    // Get the current exception if it exists
                    if let Some(exc) = &self.current_exception {
                        // Bind the exception to the variable in the current scope
                        // For now, use globals as a simple binding mechanism
                        self.globals
                            .insert(var_id as u32, Value::Condition(exc.clone()));
                    }
                }

                Instruction::ClearException => {
                    // TODO: Implement clear exception
                    // Clear current exception
                    self.current_exception = None;
                }

                Instruction::InvokeRestart => {
                    // TODO: Implement invoke restart
                    // Invoke a restart by name
                    let _restart_name_id = self.read_u16(bytecode, &mut ip);
                }
            }
        }
    }
}
