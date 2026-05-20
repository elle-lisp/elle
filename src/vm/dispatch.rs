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
                crate::value::fiberheap::reset_alloc_region();
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

        // Arm: from here, any allocation with alloc_region == IDLE panics.
        crate::value::fiberheap::arm_region_tracking();

        loop {
            // Check for pre-existing error signal (e.g., from previous Call)
            if let Some((bits, _)) = self.fiber.signal {
                if bits.contains(SIG_ERROR) || bits.contains(SIG_HALT) {
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    crate::value::fiberheap::reset_alloc_region();
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
                    // alloc_region is IDLE here (between instructions).
                    // Temporarily set to 0 so error_val allocation doesn't
                    // trigger the IDLE panic.
                    crate::value::fiberheap::set_alloc_region(crate::value::fiberheap::REGION_INFRA);
                    let err = crate::value::error_val(
                        "allocation-error",
                        format!(
                            "heap object limit exceeded ({} objects, limit {})",
                            count, limit
                        ),
                    );
                    crate::value::fiberheap::poison_alloc_region();
                    unsafe { (*heap_ptr).set_object_limit(saved_limit) };
                    self.fiber.signal = Some((SIG_ERROR, err));
                    if self.error_loc.is_none() {
                        self.error_loc = location_map.get(&instr_ip).cloned();
                    }
                    crate::value::fiberheap::reset_alloc_region();
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
                    crate::value::fiberheap::reset_alloc_region();
                    return (SIG_OK, ip);
                }

                // Call instructions — region u16 precedes arg_count
                Instruction::Call => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_call(
                        bytecode,
                        constants,
                        closure_env,
                        &mut ip,
                        instr_ip,
                        location_map,
                        false,
                    ) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::CallChecked => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_call(
                        bytecode,
                        constants,
                        closure_env,
                        &mut ip,
                        instr_ip,
                        location_map,
                        true,
                    ) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::TailCall => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_tail_call(&mut ip, bc, false) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::TailCallChecked => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_tail_call(&mut ip, bc, true) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }

                // Closures — region u16 is first operand
                Instruction::MakeClosure => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    closure::handle_make_closure(self, bc, &mut ip, consts);
                    crate::value::fiberheap::poison_alloc_region();
                }

                // Data structures
                Instruction::Pair => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_list(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::First => {
                    data::handle_first(self);
                }
                Instruction::Rest => {
                    data::handle_rest(self);
                }
                Instruction::MakeArrayMut => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_make_array(self, bc, &mut ip);
                    crate::value::fiberheap::poison_alloc_region();
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
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_array_slice_from(self, bc, &mut ip);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::StructGetOrNil => {
                    data::handle_struct_get_or_nil(self, bc, &mut ip, constants);
                }
                Instruction::StructGetDestructure => {
                    data::handle_struct_get_destructure(self, bc, &mut ip, constants);
                }
                Instruction::StructRest => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_struct_rest(self, bc, &mut ip, constants);
                    crate::value::fiberheap::poison_alloc_region();
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
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    capture::handle_make_capture(self);
                    crate::value::fiberheap::poison_alloc_region();
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

                    crate::value::fiberheap::reset_alloc_region();
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
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    super::eval::handle_eval_instruction(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::ArrayMutExtend => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_array_extend(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::ArrayMutPush => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    data::handle_array_push(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::CallArrayMut => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_call_array(
                        bytecode,
                        constants,
                        closure_env,
                        &mut ip,
                        instr_ip,
                        location_map,
                        false,
                    ) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::TailCallArrayMut => {
                    check_fuel!(self, instr_ip);
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    if let Some(bits) = self.handle_tail_call_array(&mut ip, bc, false) {
                        crate::value::fiberheap::reset_alloc_region();
                        return (bits, ip);
                    }
                    crate::value::fiberheap::poison_alloc_region();
                }

                Instruction::FreeRegion => {
                    let region_id = self.read_u16(bc, &mut ip);
                    let heap_ptr = crate::value::fiberheap::current_heap_ptr();
                    let rc = if heap_ptr.is_null() {
                        0
                    } else {
                        let rid = region_id as usize;
                        let heap = unsafe { &*heap_ptr };
                        if rid < heap.region_rc_len() { heap.region_rc_get(rid) } else { 0 }
                    };
                    if rc == 0 {
                        crate::value::fiberheap::free_region(region_id);
                        if !heap_ptr.is_null() {
                            unsafe { (*heap_ptr).regions_freed += 1 };
                        }
                    } else {
                        // Region is pinned (rc > 0) — a caller holds a
                        // reference into this region. Skip the free.
                        // The region will be freed on final decref_region.
                        if !heap_ptr.is_null() {
                            unsafe { (*heap_ptr).regions_pinned += 1 };
                        }
                        if self.runtime_config.has_trace_bit(crate::config::trace_bits::REGIONS) {
                            eprintln!(
                                "[trace:regions] FreeRegion({}) skipped: rc={} (region pinned)",
                                region_id, rc,
                            );
                        }
                    }
                }

                Instruction::DropSlot => {
                    let slot = self.read_u16(bc, &mut ip) as usize;
                    let frame_base = self.current_frame_base();
                    let abs_idx = frame_base + slot;
                    let val = self.fiber.stack[abs_idx];
                    if val.is_heap() {
                        // Decref the binding's incref (from StoreLocal),
                        // then free if rc reaches 0. Safe because DropSlot
                        // is only emitted where escape analysis proves the
                        // value hasn't escaped to collections.
                        crate::value::fiberheap::decref(val);
                        crate::value::fiberheap::drop_slot_value(val);
                        self.fiber.stack[abs_idx] = Value::NIL;
                    }
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
                            crate::value::fiberheap::set_alloc_region(crate::value::fiberheap::REGION_INFRA);
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
                            crate::value::fiberheap::poison_alloc_region();
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
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_length(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrGet => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_get(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrPut => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_put(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrDel => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_del(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrHas => {
                    types::handle_intr_has(self);
                }
                Instruction::IntrPush => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_push(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrStringPush => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_string_push(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrBytesPush => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_bytes_push(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrPop => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_pop(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrFreeze => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_freeze(self);
                    crate::value::fiberheap::poison_alloc_region();
                }
                Instruction::IntrThaw => {
                    let region_id = self.read_u16(bc, &mut ip);
                    crate::value::fiberheap::set_alloc_region(region_id);
                    types::handle_intr_thaw(self);
                    crate::value::fiberheap::poison_alloc_region();
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
                            crate::value::fiberheap::set_alloc_region(crate::value::fiberheap::REGION_INFRA);
                            let err = crate::value::error_val(
                                "signal-violation",
                                format!(
                                    "restrict: closure may emit {} but parameter is restricted to {}",
                                    excess_str, allowed_str
                                ),
                            );
                            crate::value::fiberheap::poison_alloc_region();
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
                    crate::value::fiberheap::reset_alloc_region();
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
