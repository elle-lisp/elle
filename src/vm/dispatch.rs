//! Main instruction dispatch loop.
//!
//! This module contains the core bytecode execution loop that dispatches
//! instructions to their handlers.

use crate::compiler::bytecode::Instruction;
use crate::value::{CoroutineState, Value};
use std::rc::Rc;

use super::core::{VmResult, VM};
use super::{
    arithmetic, closure, comparison, control, data, literals, scope, stack, types, variables,
};

impl VM {
    /// Inner execution loop that handles all instructions.
    /// This is the implementation; the public wrapper handles handler isolation.
    pub(super) fn execute_bytecode_inner_impl(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        let mut ip = start_ip;
        let mut instruction_count = 0;
        const MAX_INSTRUCTIONS: usize = 100000;

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

            // Check for pending exception at the START of each iteration
            if self.current_exception.is_some() && !self.handling_exception {
                if let Some(handler) = self.exception_handlers.last() {
                    while self.stack.len() > handler.stack_depth {
                        self.stack.pop();
                    }
                    self.handling_exception = true;
                    ip = handler.handler_offset as usize;
                    continue;
                } else {
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
                    return self.handle_return(bytecode, constants, closure_env, ip);
                }

                // Call instructions
                Instruction::Call => {
                    if let Some(result) =
                        self.handle_call(bytecode, constants, closure_env, &mut ip)?
                    {
                        return Ok(result);
                    }
                }
                Instruction::TailCall => {
                    if let Some(result) = self.handle_tail_call(&mut ip, bytecode)? {
                        return Ok(result);
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
                Instruction::Rem => {
                    arithmetic::handle_rem(self)?;
                }

                // Bitwise operations
                Instruction::BitAnd => {
                    arithmetic::handle_bit_and(self)?;
                }
                Instruction::BitOr => {
                    arithmetic::handle_bit_or(self)?;
                }
                Instruction::BitXor => {
                    arithmetic::handle_bit_xor(self)?;
                }
                Instruction::BitNot => {
                    arithmetic::handle_bit_not(self)?;
                }
                Instruction::Shl => {
                    arithmetic::handle_shl(self)?;
                }
                Instruction::Shr => {
                    arithmetic::handle_shr(self)?;
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

                // Scope management
                Instruction::PushScope => {
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

                // Cell operations
                Instruction::MakeCell => {
                    scope::handle_make_cell(self)?;
                }
                Instruction::UnwrapCell => {
                    scope::handle_unwrap_cell(self)?;
                }
                Instruction::UpdateCell => {
                    scope::handle_update_cell(self)?;
                }

                // Exception handling
                Instruction::PushHandler => {
                    self.handle_push_handler(bytecode, &mut ip);
                }
                Instruction::PopHandler => {
                    self.exception_handlers.pop();
                }
                Instruction::CreateHandler => {
                    let _handler_fn_idx = self.read_u16(bytecode, &mut ip);
                    let _condition_id = self.read_u16(bytecode, &mut ip);
                }
                Instruction::CheckException => {
                    if self.current_exception.is_none() {
                        return Err("CheckException reached with no exception set".to_string());
                    }
                }
                Instruction::MatchException => {
                    self.handle_match_exception(bytecode, &mut ip);
                }
                Instruction::BindException => {
                    self.handle_bind_exception(bytecode, &mut ip, constants)?;
                }
                Instruction::ClearException => {
                    self.current_exception = None;
                    self.handling_exception = false;
                }
                Instruction::ReraiseException => {
                    self.exception_handlers.pop();
                    self.handling_exception = false;
                }
                Instruction::InvokeRestart => {
                    let _restart_name_id = self.read_u16(bytecode, &mut ip);
                }
                Instruction::LoadException => {
                    self.handle_load_exception();
                }

                // Yield
                Instruction::Yield => {
                    return self.handle_yield(bytecode, constants, closure_env, ip);
                }
            }

            // Exception interrupt mechanism
            if self.current_exception.is_some() && !self.handling_exception {
                if let Some(handler) = self.exception_handlers.last() {
                    while self.stack.len() > handler.stack_depth {
                        self.stack.pop();
                    }
                    self.handling_exception = true;
                    ip = handler.handler_offset as usize;
                } else {
                    return Ok(VmResult::Done(Value::NIL));
                }
            }

            // Check for pending yield from yield-from delegation
            if let Some(yielded_value) = self.take_pending_yield() {
                return self.handle_pending_yield(
                    bytecode,
                    constants,
                    closure_env,
                    ip,
                    yielded_value,
                );
            }
        }
    }

    /// Handle the Return instruction.
    fn handle_return(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        ip: usize,
    ) -> Result<VmResult, String> {
        // Check for pending yield before returning
        if let Some(yielded_value) = self.take_pending_yield() {
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

            return Ok(VmResult::Yielded {
                value: yielded_value,
                continuation,
            });
        }

        let value = control::handle_return(self)?;
        Ok(VmResult::Done(value))
    }

    /// Handle the Yield instruction.
    fn handle_yield(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        ip: usize,
    ) -> Result<VmResult, String> {
        let yielded_value = self.stack.pop().ok_or("Stack underflow on yield")?;

        let coroutine = match self.current_coroutine() {
            Some(co) => co.clone(),
            None => return Err("yield used outside of coroutine".to_string()),
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
            co.yielded_value = None;
        }

        Ok(VmResult::Yielded {
            value: yielded_value,
            continuation,
        })
    }

    /// Handle a pending yield from yield-from delegation.
    fn handle_pending_yield(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        ip: usize,
        yielded_value: Value,
    ) -> Result<VmResult, String> {
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

        Ok(VmResult::Yielded {
            value: yielded_value,
            continuation,
        })
    }

    /// Handle the PushHandler instruction.
    fn handle_push_handler(&mut self, bytecode: &[u8], ip: &mut usize) {
        let handler_offset = self.read_u16(bytecode, ip);
        let finally_offset_val = self.read_i16(bytecode, ip);
        let finally_offset = if finally_offset_val == -1 {
            None
        } else {
            Some(finally_offset_val)
        };

        use crate::value::ExceptionHandler;
        self.exception_handlers.push(ExceptionHandler {
            handler_offset,
            finally_offset,
            stack_depth: self.stack.len(),
        });
    }

    /// Handle the MatchException instruction.
    fn handle_match_exception(&mut self, bytecode: &[u8], ip: &mut usize) {
        use super::is_exception_subclass;

        let handler_id = self.read_u16(bytecode, ip);

        let matches = if let Some(exc) = &self.current_exception {
            is_exception_subclass(exc.exception_id, handler_id as u32)
        } else {
            false
        };

        self.stack.push(Value::bool(matches));
    }

    /// Handle the BindException instruction.
    fn handle_bind_exception(
        &mut self,
        bytecode: &[u8],
        ip: &mut usize,
        constants: &[Value],
    ) -> Result<(), String> {
        let const_idx = self.read_u16(bytecode, ip) as usize;

        if let Some(exc) = &self.current_exception {
            if let Some(const_val) = constants.get(const_idx) {
                if let Some(sym_id) = const_val.as_symbol() {
                    use crate::value::heap::{alloc, HeapObject};
                    let exc_value = alloc(HeapObject::Condition((**exc).clone()));
                    self.globals.insert(sym_id, exc_value);
                } else {
                    return Err("BindException: Expected symbol in constants".to_string());
                }
            } else {
                return Err("BindException: Expected symbol in constants".to_string());
            }
        }
        Ok(())
    }

    /// Handle the LoadException instruction.
    fn handle_load_exception(&mut self) {
        let exc_value = if let Some(exc) = &self.current_exception {
            use crate::value::heap::{alloc, HeapObject};
            alloc(HeapObject::Condition((**exc).clone()))
        } else {
            Value::NIL
        };
        self.stack.push(exc_value);
    }
}
