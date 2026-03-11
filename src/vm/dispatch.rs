//! Main instruction dispatch loop.
//!
//! This module contains the core bytecode execution loop that dispatches
//! instructions to their handlers.

use crate::compiler::bytecode::Instruction;
use crate::error::LocationMap;
use crate::value::{SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
use std::rc::Rc;

use super::core::VM;
use super::{
    arithmetic, cell, closure, comparison, control, data, literals, stack, types, variables,
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
        location_map: &Rc<LocationMap>,
    ) -> (SignalBits, usize) {
        let mut ip = start_ip;
        let mut instr_ip = start_ip;

        // Deref to slices for instruction handlers
        let bc: &[u8] = bytecode;
        let consts: &[Value] = constants;

        loop {
            // Check for pre-existing error signal (e.g., from previous Call)
            if let Some((bits, _)) = self.fiber.signal {
                if bits.contains(SIG_ERROR) || bits.contains(SIG_HALT) {
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    return (bits, ip);
                }
            }

            // Check for allocation limit violation from previous instruction.
            // Temporarily remove the limit so the error struct can be allocated.
            if let Some((count, limit)) = crate::value::heap::take_alloc_error() {
                let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
                let saved_limit = if heap_ptr.is_null() {
                    crate::value::heap::heap_arena_set_object_limit(None)
                } else {
                    unsafe { (*heap_ptr).set_object_limit(None) }
                };
                let err = crate::value::error_val(
                    "allocation-error",
                    format!(
                        "heap object limit exceeded ({} objects, limit {})",
                        count, limit
                    ),
                );
                if heap_ptr.is_null() {
                    crate::value::heap::heap_arena_set_object_limit(saved_limit);
                } else {
                    unsafe { (*heap_ptr).set_object_limit(saved_limit) };
                }
                self.fiber.signal = Some((SIG_ERROR, err));
                if self.error_loc.is_none() {
                    self.error_loc = location_map.get(&instr_ip).cloned();
                }
                return (SIG_ERROR, ip);
            }

            if ip >= bc.len() {
                panic!("VM bug: Unexpected end of bytecode");
            }

            instr_ip = ip; // save instruction start before reading opcode
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

                // Dead instructions — never emitted after primitives-as-locals.
                Instruction::LoadGlobal => {
                    unreachable!("dead instruction: LoadGlobal")
                }
                Instruction::StoreGlobal => {
                    unreachable!("dead instruction: StoreGlobal")
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
                    if let Some(bits) = self.handle_call(
                        bytecode,
                        constants,
                        closure_env,
                        &mut ip,
                        instr_ip,
                        location_map,
                    ) {
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
                Instruction::MakeArrayMut => {
                    data::handle_make_array(self, bc, &mut ip);
                }
                Instruction::ArrayMutRef => {
                    data::handle_array_ref(self);
                }
                Instruction::ArrayMutSet => {
                    data::handle_array_set(self);
                }

                // Destructuring (silent nil on type mismatch)
                Instruction::CarOrNil => {
                    data::handle_car_or_nil(self);
                }
                Instruction::CdrOrNil => {
                    data::handle_cdr_or_nil(self);
                }
                Instruction::ArrayMutRefOrNil => {
                    data::handle_array_ref_or_nil(self, bc, &mut ip);
                }
                Instruction::ArrayMutSliceFrom => {
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
                Instruction::IsArrayMut => {
                    types::handle_is_array_mut(self);
                }
                Instruction::IsStruct => {
                    types::handle_is_struct(self);
                }
                Instruction::IsTable => {
                    types::handle_is_table(self);
                }
                Instruction::ArrayMutLen => {
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

                // Dead instructions — never emitted by the LIR emitter.
                // Panic immediately if encountered (indicates bytecode bug).
                Instruction::PushScope => {
                    panic!("VM bug: PushScope is a dead instruction — never emitted");
                }
                Instruction::PopScope => {
                    panic!("VM bug: PopScope is a dead instruction — never emitted");
                }
                Instruction::DefineLocal => {
                    panic!("VM bug: DefineLocal is a dead instruction — never emitted");
                }

                // Box operations
                Instruction::MakeLBox => {
                    cell::handle_make_lbox(self);
                }
                Instruction::UnlBox => {
                    cell::handle_unlbox(self);
                }
                Instruction::UpdateLBox => {
                    cell::handle_update_lbox(self);
                }

                // Yield — capture suspended frame and suspend
                Instruction::Yield => {
                    return self.handle_yield(bytecode, constants, closure_env, ip, location_map);
                }

                // Runtime eval — compile and execute a datum
                Instruction::Eval => {
                    super::eval::handle_eval_instruction(self);
                }
                Instruction::ArrayMutExtend => {
                    data::handle_array_extend(self);
                }
                Instruction::ArrayMutPush => {
                    data::handle_array_push(self);
                }
                Instruction::CallArrayMut => {
                    if let Some(bits) = self.handle_call_array(
                        bytecode,
                        constants,
                        closure_env,
                        &mut ip,
                        instr_ip,
                        location_map,
                    ) {
                        return (bits, ip);
                    }
                }
                Instruction::TailCallArrayMut => {
                    if let Some(bits) = self.handle_tail_call_array(&mut ip, bc) {
                        return (bits, ip);
                    }
                }

                // Allocation region markers: push/pop scope marks on FiberHeap.
                // No-op for root fiber (no FiberHeap installed).
                Instruction::RegionEnter => {
                    crate::value::fiber_heap::region_enter();
                }
                Instruction::RegionExit => {
                    crate::value::fiber_heap::region_exit();
                }

                // Dynamic parameter frame management
                Instruction::PushParamFrame => {
                    let count = bc[ip] as usize;
                    ip += 1;
                    let mut frame = Vec::with_capacity(count);
                    // Stack has pairs pushed as [param1, val1, param2, val2, ...]
                    // We need to pop them in reverse order (last pair first)
                    // First collect all pairs from the stack
                    let mut raw_pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let val = self
                            .fiber
                            .stack
                            .pop()
                            .expect("VM bug: stack underflow in PushParamFrame");
                        let param = self
                            .fiber
                            .stack
                            .pop()
                            .expect("VM bug: stack underflow in PushParamFrame");
                        raw_pairs.push((param, val));
                    }
                    // Process in reverse to restore original order
                    for (param, val) in raw_pairs.into_iter().rev() {
                        if let Some((id, _default)) = param.as_parameter() {
                            frame.push((id, val));
                        } else {
                            use crate::value::error_val;
                            self.fiber.signal = Some((
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!(
                                        "parameterize: {} is not a parameter",
                                        param.type_name()
                                    ),
                                ),
                            ));
                            self.fiber.stack.push(Value::NIL);
                            break;
                        }
                    }
                    if self.fiber.signal.is_none() {
                        self.fiber.param_frames.push(frame);
                    }
                }
                Instruction::PopParamFrame => {
                    self.fiber.param_frames.pop();
                }
                Instruction::IsSet => {
                    types::handle_is_set(self);
                }
                Instruction::IsSetMut => {
                    types::handle_is_set_mut(self);
                }
                Instruction::CheckEffectBound => {
                    // Read u32 as two u16s (low half first, then high half)
                    let lo = self.read_u16(bc, &mut ip) as u32;
                    let hi = self.read_u16(bc, &mut ip) as u32;
                    let mut allowed_bits = lo | (hi << 16);
                    // SIG_YIELD is the delivery mechanism for all signals.
                    // If the bound allows ANY signal, implicitly allow SIG_YIELD.
                    if allowed_bits != 0 {
                        allowed_bits |= SIG_YIELD.0;
                    }
                    let val = self.fiber.stack.pop().unwrap_or(Value::NIL);
                    if let Some(closure) = val.as_closure() {
                        let effect_bits = closure.effect().bits.0;
                        let excess = effect_bits & !allowed_bits;
                        if excess != 0 {
                            let registry =
                                crate::effects::registry::global_registry().lock().unwrap();
                            let excess_str = registry
                                .format_signal_bits(crate::value::fiber::SignalBits(excess));
                            let allowed_str = registry
                                .format_signal_bits(crate::value::fiber::SignalBits(allowed_bits));
                            let err = crate::value::error_val(
                                "effect-violation",
                                format!(
                                    "restrict: closure may emit {} but parameter is restricted to {}",
                                    excess_str, allowed_str
                                ),
                            );
                            self.fiber.signal = Some((SIG_ERROR, err));
                        }
                    } else {
                        // Non-closure values (primitives, etc.) are inert — they pass
                        // any effect bound check. Only closures carry effect metadata.
                    }
                }
            }

            // Check for error signal set by this instruction's handler
            if let Some((bits, _)) = self.fiber.signal {
                if bits.contains(SIG_ERROR) || bits.contains(SIG_HALT) {
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    return (bits, ip);
                }
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
        location_map: &Rc<LocationMap>,
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
            active_allocator: crate::value::fiber_heap::save_active_allocator(),
            location_map: location_map.clone(),
        };

        self.fiber.signal = Some((SIG_YIELD, yielded_value));
        self.fiber.suspended = Some(vec![frame]);
        (SIG_YIELD, ip)
    }
}
