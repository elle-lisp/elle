//! The bytecode dispatch match.
//!
//! Extracted verbatim from the inner execution loop so the loop root stays a
//! thin fuel/signal/decode harness. `dispatch_instruction` is the single arm of
//! that harness: it routes one already-decoded instruction to its handler.
//!
//! `#[inline]` is load-bearing — the loop is hot and the match must fold back
//! into the caller so the harness's fall-through and the handlers stay in one
//! function body, exactly as before the split.
//!
//! Contract: `Some((bits, ip))` means the enclosing loop must return that value
//! (exit dispatch); `None` means fall through to the loop's post-handler signal
//! check and continue. `ip` is advanced in place; `instr_ip` is the opcode's
//! start (for error-location attribution).
//!
//! Fuel is charged inline in the branch/call arms via `charge_fuel` — the
//! single opcode match is the one place that inspects the instruction, so the
//! accounting stays on the same jump table as dispatch (no separate pre-match).

use super::*;

impl VM {
    /// Charge one unit of fuel for a branch/call opcode. Returns `Some(exit)`
    /// when fuel is exhausted — the caller must propagate it out of the dispatch
    /// loop — and `None` after decrementing. `resume_ip` must be the opcode start
    /// (`instr_ip`), so a fuel-yielded fiber re-runs the whole opcode on resume.
    /// When fuel is `None` (the common, unmetered case) the `if let` is skipped —
    /// a not-taken branch, negligible overhead on the hot path.
    #[inline]
    fn charge_fuel(&mut self, resume_ip: usize) -> Option<(SignalBits, usize)> {
        if let Some(ref mut fuel) = self.fiber.fuel {
            if *fuel == 0 {
                self.fiber.signal = Some((SIG_FUEL, Value::NIL));
                return Some((SIG_FUEL, resume_ip));
            }
            *fuel -= 1;
        }
        None
    }

    // The arg list is the inner loop's live context (bytecode/const slices, the
    // Rc handles call/yield capture, the in/out `ip`), threaded in wholesale so
    // the match stays a verbatim extraction of the old inline body.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub(super) fn dispatch_instruction(
        &mut self,
        instr: Instruction,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        bc: &[u8],
        consts: &[Value],

        locations: crate::value::closure::LocationTable<'_>,
        ip: &mut usize,
        instr_ip: usize,
    ) -> Option<(SignalBits, usize)> {
        match instr {
            // Stack operations
            Instruction::LoadConst => {
                stack::handle_load_const(self, bc, ip, consts);
            }
            Instruction::LoadLocal => {
                stack::handle_load_local(self, bc, ip);
            }
            Instruction::Pop => {
                stack::handle_pop(self);
            }
            Instruction::Dup => {
                stack::handle_dup(self);
            }
            Instruction::DupN => {
                stack::handle_dup_n(self, bc, ip);
            }

            Instruction::StoreLocal => {
                variables::handle_store_local(self, bc, ip);
            }
            Instruction::LoadUpvalue => {
                variables::handle_load_upvalue(self, bc, ip, Some(closure_env));
            }
            Instruction::LoadUpvalueRaw => {
                variables::handle_load_upvalue_raw(self, bc, ip, Some(closure_env));
            }
            Instruction::StoreUpvalue => {
                variables::handle_store_upvalue(self, bc, ip, Some(closure_env));
            }
            Instruction::LoadSelf => {
                variables::handle_load_self(self);
            }

            // Control flow
            Instruction::Jump => {
                // Only backward jumps (loops) burn fuel. Peek the big-endian
                // i32 offset without consuming bytes (handle_jump re-reads them),
                // charging fuel before the jump.
                let offset = i32::from_be_bytes([bc[*ip], bc[*ip + 1], bc[*ip + 2], bc[*ip + 3]]);
                if offset < 0 {
                    if let Some(exit) = self.charge_fuel(instr_ip) {
                        return Some(exit);
                    }
                }
                control::handle_jump(bc, ip, self);
            }
            Instruction::JumpIfFalse => {
                control::handle_jump_if_false(bc, ip, self);
            }
            Instruction::JumpIfTrue => {
                control::handle_jump_if_true(bc, ip, self);
            }
            Instruction::Return => {
                let value = control::handle_return(self);
                self.fiber.signal = Some((SIG_OK, value));
                return Some((SIG_OK, *ip));
            }

            // Call instructions charge fuel before invoking the callee.
            Instruction::Call => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_call(code, closure_env, ip, instr_ip, false) {
                    return Some((bits, *ip));
                }
            }
            Instruction::CallChecked => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_call(code, closure_env, ip, instr_ip, true) {
                    return Some((bits, *ip));
                }
            }
            Instruction::TailCall => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_tail_call(ip, bc, false) {
                    return Some((bits, *ip));
                }
            }
            Instruction::TailCallChecked => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_tail_call(ip, bc, true) {
                    return Some((bits, *ip));
                }
            }

            // Closures
            Instruction::MakeClosure => {
                let region = self.read_static_region(bc, ip);
                closure::handle_make_closure(
                    self,
                    bc,
                    ip,
                    code.child_protos(),
                    region,
                    code.merged_slots(),
                );
            }

            // Data structures
            Instruction::Pair => {
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                data::handle_list(self, region_id);
            }
            Instruction::First => {
                data::handle_first(self);
            }
            Instruction::Rest => {
                data::handle_rest(self);
            }
            Instruction::MakeArrayMut => {
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                data::handle_make_array(self, bc, ip, region_id);
            }
            Instruction::MaterializeConst => {
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                literals::handle_materialize_const(self, bc, ip, region_id);
            }
            Instruction::ArrayMutRef => {
                data::handle_array_ref(self);
            }
            Instruction::ArrayMutSet => {
                data::handle_array_set(self);
            }

            // Destructuring
            Instruction::MatchFail => {
                data::handle_match_fail(self);
            }
            Instruction::FirstDestructure => {
                data::handle_car_destructure(self);
            }
            Instruction::RestDestructure => {
                data::handle_cdr_destructure(self);
            }
            Instruction::ArrayMutRefDestructure => {
                data::handle_array_ref_destructure(self, bc, ip);
            }
            Instruction::ArrayMutSliceFrom => {
                data::handle_array_slice_from(self, bc, ip);
            }
            Instruction::StructGetOrNil => {
                data::handle_struct_get_or_nil(self, bc, ip, consts);
            }
            Instruction::StructGetDestructure => {
                data::handle_struct_get_destructure(self, bc, ip, consts);
            }
            Instruction::StructRest => {
                data::handle_struct_rest(self, bc, ip, consts);
            }

            // Silent destructuring (parameter context: absent optional params → nil)
            Instruction::FirstOrNil => {
                data::handle_car_or_nil(self);
            }
            Instruction::RestOrNil => {
                data::handle_cdr_or_nil(self);
            }
            Instruction::ArrayMutRefOrNil => {
                data::handle_array_ref_or_nil(self, bc, ip);
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
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                capture::handle_make_capture(self, region_id);
            }
            Instruction::UnwrapCapture => {
                capture::handle_unwrap_capture(self);
            }
            Instruction::UpdateCapture => {
                if crate::value::fiberheap::freelog::enabled() {
                    let loc = locations
                        .get(instr_ip)
                        .map(|l| format!("{l}"))
                        .unwrap_or_else(|| "?".to_string());
                    crate::value::fiberheap::freelog::set_context(format!("UpdateCapture @ {loc}"));
                }
                capture::handle_update_capture(self);
            }

            // Emit — exit dispatch loop for all signals.
            // SIG_ERROR: store error, no SuspendedFrame (error propagation).
            // Other signals: create SuspendedFrame (cooperative suspension).
            Instruction::Emit => {
                let signal_bits = self.read_signal_bits(bc, ip);
                return Some(self.handle_emit(signal_bits, code, closure_env, *ip));
            }

            // Runtime eval — compile and execute a datum
            Instruction::Eval => {
                crate::vm::eval::handle_eval_instruction(self);
            }
            Instruction::ArrayMutExtend => {
                data::handle_array_extend(self);
            }
            Instruction::ArrayMutPush => {
                data::handle_array_push(self);
            }
            Instruction::CallArrayMut => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_call_array(code, closure_env, ip, instr_ip, false) {
                    return Some((bits, *ip));
                }
            }
            Instruction::TailCallArrayMut => {
                if let Some(exit) = self.charge_fuel(instr_ip) {
                    return Some(exit);
                }
                if let Some(bits) = self.handle_tail_call_array(ip, bc, false) {
                    return Some((bits, *ip));
                }
            }

            Instruction::IncrefRegion => {
                region::handle_incref_region(self, bc, ip);
            }

            Instruction::DecrefRegion => {
                region::handle_decref_region(self, bc, ip, locations, instr_ip);
            }

            Instruction::DecrefValueRegion => {
                region::handle_decref_value_region(self, locations, instr_ip);
            }

            Instruction::DecrefCellRegion => {
                region::handle_decref_cell_region(self, locations, instr_ip);
            }

            Instruction::IncrefValueRegion => {
                region::handle_incref_value_region(self, locations, instr_ip);
            }

            Instruction::AdoptRegion => {
                region::handle_adopt_region(self);
            }

            Instruction::AdoptCellRegion => {
                region::handle_adopt_cell_region(self);
            }

            Instruction::AdoptIntoActivation => {
                region::handle_adopt_into_activation(self);
            }

            Instruction::FreeRegionGroup => {
                region::handle_free_region_group(self, bc, ip);
            }

            Instruction::AssertRegionMatches => {
                region::handle_assert_region_matches(self, bc, ip);
            }

            // Dynamic parameter frame management
            Instruction::PushParamFrame => {
                self.handle_push_param_frame(bc, ip);
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
            Instruction::IntrStringPush => {
                types::handle_intr_string_push(self);
            }
            Instruction::IntrBytesPush => {
                types::handle_intr_bytes_push(self);
            }
            Instruction::IntrPop => {
                types::handle_intr_pop(self);
            }
            Instruction::IntrFreeze => {
                // IntrFreeze allocates a fresh immutable container
                // holding the source's entries. The lowerer's
                // emit_alloc assigns it a region (so a matching
                // DecrefRegion fires at scope exit); the runtime
                // must alloc into that same region or the decref
                // hits an empty slot and the phantom-region
                // debug_assert panics.
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                types::handle_intr_freeze(self, region_id);
            }
            Instruction::IntrThaw => {
                // Same accounting as IntrFreeze: the new mutable
                // container must land in the lowerer-assigned
                // region.
                let region = self.read_static_region(bc, ip);
                let region_id =
                    self.runtime_region_for_alloc_slot_maybe_merged(region, code.merged_slots());
                types::handle_intr_thaw(self, region_id);
            }
            Instruction::Identical => {
                types::handle_identical(self);
            }
            Instruction::CheckSignalBound => {
                self.handle_check_signal_bound(bc, ip);
            }
        }

        None
    }
}
