//! Main instruction dispatch loop.
//!
//! This module contains the core bytecode execution loop that dispatches
//! instructions to their handlers.

use crate::compiler::bytecode::Instruction;
use crate::value::{SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
use std::rc::Rc;

use super::core::VM;
use super::{
    arithmetic, closure, comparison, control, data, literals, scope, stack, types, variables,
};

impl VM {
    /// Inner execution loop that handles all instructions.
    ///
    /// Takes `Rc` references to bytecode and constants so that yield and
    /// call handlers can capture them cheaply (Rc clone, not data copy).
    /// Derefs to slices for individual instruction handlers.
    ///
    /// Returns `(SignalBits, ip)` — the signal and the IP at exit.
    pub(super) fn execute_bytecode_inner_impl(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        start_ip: usize,
    ) -> (SignalBits, usize) {
        let mut ip = start_ip;

        // Deref to slices for instruction handlers
        let bc: &[u8] = bytecode;
        let consts: &[Value] = constants;

        loop {
            // If an error or halt signal is pending, propagate immediately.
            if let Some((bits @ (SIG_ERROR | SIG_HALT), _)) = self.fiber.signal {
                return (bits, ip);
            }

            if ip >= bc.len() {
                panic!("VM bug: Unexpected end of bytecode");
            }

            let instr_byte = bc[ip];
            ip += 1;

            let instr: Instruction = unsafe { std::mem::transmute(instr_byte) };

            match instr {
                // Stack operations
                Instruction::LoadConst => {
                    stack::handle_load_const(self, bc, &mut ip, consts);
                }
                Instruction::LoadLocal => {
                    stack::handle_load_local(self, bc, &mut ip);
                }
                Instruction::Pop => {
                    stack::handle_pop(self);
                }
                Instruction::Dup => {
                    stack::handle_dup(self);
                }
                Instruction::DupN => {
                    stack::handle_dup_n(self, bc, &mut ip);
                }

                // Variable access
                Instruction::LoadGlobal => {
                    variables::handle_load_global(self, bc, &mut ip, consts);
                }
                Instruction::StoreGlobal => {
                    variables::handle_store_global(self, bc, &mut ip, consts);
                }
                Instruction::StoreLocal => {
                    variables::handle_store_local(self, bc, &mut ip);
                }
                Instruction::LoadUpvalue => {
                    variables::handle_load_upvalue(self, bc, &mut ip, Some(closure_env));
                }
                Instruction::LoadUpvalueRaw => {
                    variables::handle_load_upvalue_raw(self, bc, &mut ip, Some(closure_env));
                }
                Instruction::StoreUpvalue => {
                    variables::handle_store_upvalue(self, bc, &mut ip, Some(closure_env));
                }

                // Control flow
                Instruction::Jump => {
                    control::handle_jump(bc, &mut ip, self);
                }
                Instruction::JumpIfFalse => {
                    control::handle_jump_if_false(bc, &mut ip, self);
                }
                Instruction::JumpIfTrue => {
                    control::handle_jump_if_true(bc, &mut ip, self);
                }
                Instruction::Return => {
                    let value = control::handle_return(self);
                    self.fiber.signal = Some((SIG_OK, value));
                    return (SIG_OK, ip);
                }

                // Call instructions
                Instruction::Call => {
                    if let Some(bits) = self.handle_call(bytecode, constants, closure_env, &mut ip)
                    {
                        return (bits, ip);
                    }
                }
                Instruction::TailCall => {
                    if let Some(bits) = self.handle_tail_call(&mut ip, bc) {
                        return (bits, ip);
                    }
                }

                // Closures
                Instruction::MakeClosure => {
                    closure::handle_make_closure(self, bc, &mut ip, consts);
                }

                // Data structures
                Instruction::Cons => {
                    data::handle_cons(self);
                }
                Instruction::Car => {
                    data::handle_car(self);
                }
                Instruction::Cdr => {
                    data::handle_cdr(self);
                }
                Instruction::MakeArray => {
                    data::handle_make_array(self, bc, &mut ip);
                }
                Instruction::ArrayRef => {
                    data::handle_array_ref(self);
                }
                Instruction::ArraySet => {
                    data::handle_array_set(self);
                }

                // Destructuring (silent nil on type mismatch)
                Instruction::CarOrNil => {
                    data::handle_car_or_nil(self);
                }
                Instruction::CdrOrNil => {
                    data::handle_cdr_or_nil(self);
                }
                Instruction::ArrayRefOrNil => {
                    data::handle_array_ref_or_nil(self, bc, &mut ip);
                }
                Instruction::ArraySliceFrom => {
                    data::handle_array_slice_from(self, bc, &mut ip);
                }
                Instruction::TableGetOrNil => {
                    data::handle_table_get_or_nil(self, bc, &mut ip, constants);
                }

                // Arithmetic (integer)
                Instruction::AddInt => {
                    arithmetic::handle_add_int(self);
                }
                Instruction::SubInt => {
                    arithmetic::handle_sub_int(self);
                }
                Instruction::MulInt => {
                    arithmetic::handle_mul_int(self);
                }
                Instruction::DivInt => {
                    arithmetic::handle_div_int(self);
                }

                // Arithmetic (polymorphic)
                Instruction::Add => {
                    arithmetic::handle_add(self);
                }
                Instruction::Sub => {
                    arithmetic::handle_sub(self);
                }
                Instruction::Mul => {
                    arithmetic::handle_mul(self);
                }
                Instruction::Div => {
                    arithmetic::handle_div(self);
                }
                Instruction::Rem => {
                    arithmetic::handle_rem(self);
                }

                // Bitwise operations
                Instruction::BitAnd => {
                    arithmetic::handle_bit_and(self);
                }
                Instruction::BitOr => {
                    arithmetic::handle_bit_or(self);
                }
                Instruction::BitXor => {
                    arithmetic::handle_bit_xor(self);
                }
                Instruction::BitNot => {
                    arithmetic::handle_bit_not(self);
                }
                Instruction::Shl => {
                    arithmetic::handle_shl(self);
                }
                Instruction::Shr => {
                    arithmetic::handle_shr(self);
                }

                // Comparisons
                Instruction::Eq => {
                    comparison::handle_eq(self);
                }
                Instruction::Lt => {
                    comparison::handle_lt(self);
                }
                Instruction::Gt => {
                    comparison::handle_gt(self);
                }
                Instruction::Le => {
                    comparison::handle_le(self);
                }
                Instruction::Ge => {
                    comparison::handle_ge(self);
                }

                // Type checks
                Instruction::IsNil => {
                    types::handle_is_nil(self);
                }
                Instruction::IsEmptyList => {
                    types::handle_is_empty_list(self);
                }
                Instruction::IsPair => {
                    types::handle_is_pair(self);
                }
                Instruction::IsArray => {
                    types::handle_is_array(self);
                }
                Instruction::IsTable => {
                    types::handle_is_table(self);
                }
                Instruction::ArrayLen => {
                    types::handle_array_len(self);
                }
                Instruction::IsNumber => {
                    types::handle_is_number(self);
                }
                Instruction::IsSymbol => {
                    types::handle_is_symbol(self);
                }
                Instruction::Not => {
                    types::handle_not(self);
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
                    let scope_type_byte = bc[ip];
                    ip += 1;
                    scope::handle_push_scope(self, scope_type_byte);
                }
                Instruction::PopScope => {
                    scope::handle_pop_scope(self);
                }
                Instruction::DefineLocal => {
                    scope::handle_define_local(self, bc, &mut ip, consts);
                }

                // Cell operations
                Instruction::MakeCell => {
                    scope::handle_make_cell(self);
                }
                Instruction::UnwrapCell => {
                    scope::handle_unwrap_cell(self);
                }
                Instruction::UpdateCell => {
                    scope::handle_update_cell(self);
                }

                // Yield — capture suspended frame and suspend
                Instruction::Yield => {
                    return self.handle_yield(bytecode, constants, closure_env, ip);
                }

                // Runtime eval — compile and execute a datum
                Instruction::Eval => {
                    super::eval::handle_eval_instruction(self);
                }
            }

            // If an error or halt signal was set by the instruction, propagate.
            if let Some((bits @ (SIG_ERROR | SIG_HALT), _)) = self.fiber.signal {
                return (bits, ip);
            }
        }
    }

    /// Handle the Yield instruction.
    ///
    /// Captures a SuspendedFrame (bytecode, constants, env, IP, stack)
    /// so that resume can continue from this exact point. Each call level
    /// appends its frame to the suspended chain.
    fn handle_yield(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: usize,
    ) -> (SignalBits, usize) {
        let yielded_value = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on yield");

        let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();

        let frame = SuspendedFrame {
            bytecode: bytecode.clone(),
            constants: constants.clone(),
            env: closure_env.clone(),
            ip,
            stack: saved_stack,
        };

        self.fiber.signal = Some((SIG_YIELD, yielded_value));
        self.fiber.suspended = Some(vec![frame]);
        (SIG_YIELD, ip)
    }
}
