//! Main instruction dispatch loop.
//!
//! This module contains the core bytecode execution loop that dispatches
//! instructions to their handlers.

use crate::compiler::bytecode::Instruction;
use crate::error::LocationMap;
use crate::value::{
    BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_FUEL, SIG_HALT, SIG_OK,
};
use std::rc::Rc;

use super::core::VM;
use super::{
    arithmetic, capture, closure, comparison, control, data, literals, stack, types, variables,
};

/// Decrement fuel and return from the dispatch loop if the budget is exhausted.
///
/// `$self`      — the `&mut VM` (i.e. `self` inside `execute_bytecode_inner_impl`)
/// `$resume_ip` — the opcode-start IP to resume from after refueling.
///                **Must always be `instr_ip`** (not `ip`) so that resume
///                re-executes the full instruction from scratch.
///
/// When fuel is `None` (the common case), the inner `if let` is not taken —
/// branch predicted not-taken, negligible overhead.
macro_rules! check_fuel {
    ($self:expr, $resume_ip:expr) => {
        if let Some(ref mut fuel) = $self.fiber.fuel {
            if *fuel == 0 {
                $self.fiber.signal = Some((SIG_FUEL, Value::NIL));
                return (SIG_FUEL, $resume_ip);
            }
            *fuel -= 1;
        }
    };
}

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
            // The error flag is stored on the current FiberHeap (always installed
            // after chunk 1). Temporarily remove the limit so the error struct
            // can be allocated.
            {
                let heap_ptr = crate::value::fiberheap::current_heap_ptr();
                let alloc_err = if !heap_ptr.is_null() {
                    unsafe { (*heap_ptr).take_alloc_error() }
                } else {
                    None
                };
                if let Some((count, limit)) = alloc_err {
                    let saved_limit = unsafe { (*heap_ptr).set_object_limit(None) };
                    let err = crate::value::error_val(
                        "allocation-error",
                        format!(
                            "heap object limit exceeded ({} objects, limit {})",
                            count, limit
                        ),
                    );
                    unsafe { (*heap_ptr).set_object_limit(saved_limit) };
                    self.fiber.signal = Some((SIG_ERROR, err));
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    return (SIG_ERROR, ip);
                }
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
                    // Peek offset (big-endian i32, matching read_i32) to determine
                    // direction WITHOUT consuming bytes — handle_jump re-reads them.
                    let offset = i32::from_be_bytes([bc[ip], bc[ip + 1], bc[ip + 2], bc[ip + 3]]);
                    if offset < 0 {
                        check_fuel!(self, instr_ip);
                    }
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
                    check_fuel!(self, instr_ip);
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
                    check_fuel!(self, instr_ip);
                    if let Some(bits) = self.handle_tail_call(&mut ip, bc) {
                        return (bits, ip);
                    }
                }

                // Closures
                Instruction::MakeClosure => {
                    closure::handle_make_closure(self, bc, &mut ip, consts);
                }

                // Data structures
                Instruction::Pair => {
                    data::handle_list(self);
                }
                Instruction::First => {
                    data::handle_first(self);
                }
                Instruction::Rest => {
                    data::handle_rest(self);
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

                // Destructuring
                Instruction::FirstDestructure => {
                    data::handle_car_destructure(self);
                }
                Instruction::RestDestructure => {
                    data::handle_cdr_destructure(self);
                }
                Instruction::ArrayMutRefDestructure => {
                    data::handle_array_ref_destructure(self, bc, &mut ip);
                }
                Instruction::ArrayMutSliceFrom => {
                    data::handle_array_slice_from(self, bc, &mut ip);
                }
                Instruction::StructGetOrNil => {
                    data::handle_struct_get_or_nil(self, bc, &mut ip, constants);
                }
                Instruction::StructGetDestructure => {
                    data::handle_struct_get_destructure(self, bc, &mut ip, constants);
                }
                Instruction::StructRest => {
                    data::handle_struct_rest(self, bc, &mut ip, constants);
                }

                // Silent destructuring (parameter context: absent optional params → nil)
                Instruction::FirstOrNil => {
                    data::handle_car_or_nil(self);
                }
                Instruction::RestOrNil => {
                    data::handle_cdr_or_nil(self);
                }
                Instruction::ArrayMutRefOrNil => {
                    data::handle_array_ref_or_nil(self, bc, &mut ip);
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

                // Type conversions
                Instruction::IntToFloat => {
                    arithmetic::handle_int_to_float(self);
                }
                Instruction::FloatToInt => {
                    arithmetic::handle_float_to_int(self);
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
                Instruction::IsStructMut => {
                    types::handle_is_struct_mut(self);
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

                // Box operations
                Instruction::MakeCapture => {
                    capture::handle_make_capture(self);
                }
                Instruction::UnwrapCapture => {
                    capture::handle_unwrap_capture(self);
                }
                Instruction::UpdateCapture => {
                    capture::handle_update_capture(self);
                }

                // Emit — exit dispatch loop for all signals.
                // SIG_ERROR: store error, no SuspendedFrame (error propagation).
                // Other signals: create SuspendedFrame (cooperative suspension).
                Instruction::Emit => {
                    let bits_raw = self.read_u16(bc, &mut ip) as u64;
                    let signal_bits = crate::value::fiber::SignalBits::new(bits_raw);
                    return self.handle_emit(
                        signal_bits,
                        bytecode,
                        constants,
                        closure_env,
                        ip,
                        location_map,
                    );
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
                    check_fuel!(self, instr_ip);
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
                    check_fuel!(self, instr_ip);
                    if let Some(bits) = self.handle_tail_call_array(&mut ip, bc) {
                        return (bits, ip);
                    }
                }

                // Allocation region markers: push/pop scope marks on FiberHeap.
                // Effective for both root and child fibers.
                Instruction::RegionEnter => {
                    crate::value::fiberheap::region_enter();
                }
                Instruction::RegionExit => {
                    crate::value::fiberheap::region_exit();
                }
                Instruction::RegionExitCall => {
                    crate::value::fiberheap::region_exit_call();
                }
                Instruction::RegionRotate => {
                    crate::value::fiberheap::region_rotate();
                }
                Instruction::RegionRotateDealloc => {
                    crate::value::fiberheap::region_rotate_dealloc();
                }
                Instruction::RegionRotateRefcounted => {
                    crate::value::fiberheap::region_rotate_refcounted();
                }
                Instruction::RegionExitRefcounted => {
                    crate::value::fiberheap::region_exit_refcounted();
                }

                // Outbox routing: toggle allocation target for yield-bound values.
                Instruction::OutboxEnter => {
                    crate::value::fiberheap::outbox_enter();
                }
                Instruction::OutboxExit => {
                    crate::value::fiberheap::outbox_exit();
                }

                // Explicit rotation: push/rotate/pop a flip frame.
                Instruction::FlipEnter => {
                    crate::value::fiberheap::flip_enter();
                }
                Instruction::FlipSwap => {
                    crate::value::fiberheap::flip_swap();
                }
                Instruction::FlipExit => {
                    crate::value::fiberheap::flip_exit();
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
                // New intrinsic opcodes
                Instruction::Ne => {
                    types::handle_ne(self);
                }
                Instruction::BitNotIntr => {
                    types::handle_bit_not_intr(self);
                }
                Instruction::IsBool => {
                    types::handle_is_bool(self);
                }
                Instruction::IsInt => {
                    types::handle_is_int(self);
                }
                Instruction::IsFloat => {
                    types::handle_is_float(self);
                }
                Instruction::IsString => {
                    types::handle_is_string(self);
                }
                Instruction::IsKeyword => {
                    types::handle_is_keyword(self);
                }
                Instruction::IsBytes => {
                    types::handle_is_bytes(self);
                }
                Instruction::IsBox => {
                    types::handle_is_box(self);
                }
                Instruction::IsClosure => {
                    types::handle_is_closure(self);
                }
                Instruction::IsFiber => {
                    types::handle_is_fiber(self);
                }
                Instruction::TypeOf => {
                    types::handle_type_of(self);
                }
                Instruction::Length => {
                    types::handle_length(self);
                }
                Instruction::IntrGet => {
                    types::handle_intr_get(self);
                }
                Instruction::IntrPut => {
                    types::handle_intr_put(self);
                }
                Instruction::IntrDel => {
                    types::handle_intr_del(self);
                }
                Instruction::IntrHas => {
                    types::handle_intr_has(self);
                }
                Instruction::IntrPush => {
                    types::handle_intr_push(self);
                }
                Instruction::IntrPop => {
                    types::handle_intr_pop(self);
                }
                Instruction::IntrFreeze => {
                    types::handle_intr_freeze(self);
                }
                Instruction::IntrThaw => {
                    types::handle_intr_thaw(self);
                }
                Instruction::Identical => {
                    types::handle_identical(self);
                }
                Instruction::CheckSignalBound => {
                    // Read SignalBits as four u16s (least-significant first)
                    let w0 = self.read_u16(bc, &mut ip) as u64;
                    let w1 = self.read_u16(bc, &mut ip) as u64;
                    let w2 = self.read_u16(bc, &mut ip) as u64;
                    let w3 = self.read_u16(bc, &mut ip) as u64;
                    let allowed_bits = SignalBits::new(w0 | (w1 << 16) | (w2 << 32) | (w3 << 48));
                    let val = self.fiber.stack.pop().unwrap_or(Value::NIL);
                    if let Some(closure) = val.as_closure() {
                        let signal_bits = closure.signal().bits;
                        let excess = signal_bits.subtract(allowed_bits);
                        if !excess.is_empty() {
                            let registry =
                                crate::signals::registry::global_registry().lock().unwrap();
                            let excess_str = registry.format_signal_bits(excess);
                            let allowed_str = registry.format_signal_bits(allowed_bits);
                            let err = crate::value::error_val(
                                "signal-violation",
                                format!(
                                    "restrict: closure may emit {} but parameter is restricted to {}",
                                    excess_str, allowed_str
                                ),
                            );
                            self.fiber.signal = Some((SIG_ERROR, err));
                        }
                    }
                    // Non-closure values (primitives, etc.) are silent — they pass
                    // any signal bound check. Only closures carry signal metadata.
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

    /// Handle the Emit instruction.
    ///
    /// For error signals (SIG_ERROR): stores the error in fiber.signal and
    /// exits the dispatch loop. No SuspendedFrame — errors propagate through
    /// the normal return/unwind path.
    ///
    /// For suspension signals (SIG_YIELD, user-defined): captures a
    /// SuspendedFrame so that resume can continue from this exact point.
    fn handle_emit(
        &mut self,
        signal_bits: SignalBits,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> (SignalBits, usize) {
        let value = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on emit");

        self.fiber.signal = Some((signal_bits, value));

        if !signal_bits.contains(SIG_ERROR) {
            // Suspension: save stack and create a frame for later resumption.
            let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();

            let frame = SuspendedFrame::Bytecode(BytecodeFrame {
                bytecode: bytecode.clone(),
                constants: constants.clone(),
                env: closure_env.clone(),
                ip,
                stack: saved_stack,
                location_map: location_map.clone(),
                push_resume_value: true,
            });
            self.fiber.suspended = Some(vec![frame]);
        }

        (signal_bits, ip)
    }
}
