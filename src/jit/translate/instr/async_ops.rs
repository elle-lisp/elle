use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Destructure, eval, suspending-call, and array-mut instructions (chain link).
    pub(super) fn translate_instr_async(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::LoadResumeValue { dst } => {
                // Resume goes through the interpreter. Emit NIL as dead code.
                let nil_t = builder.ins().iconst(I64, TAG_NIL as i64);
                let zero = builder.ins().iconst(I64, 0);
                self.def_var_pair(builder, dst.0, nil_t, zero);
            }

            LirInstr::MatchFail { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("MatchFail without vm pointer".to_string())
                })?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.match_fail, st, sp, vm)?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::FirstDestructure { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("FirstDestructure without vm pointer".to_string())
                })?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.first_destructure, st, sp, vm)?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::RestDestructure { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("RestDestructure without vm pointer".to_string())
                })?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.rest_destructure, st, sp, vm)?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutRefDestructure without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_ref_destructure, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutSliceFrom without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_slice_from, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsArray { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_array, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsArrayMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_array_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsStruct { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_struct, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsStructMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_struct_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutLen { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.array_len, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructGetOrNil { dst, src, key } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (kt, kp) = self.translate_const(builder, key);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructGetOrNil without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_get_or_nil, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, kt, kp, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructGetDestructure { dst, src, key } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (kt, kp) = self.translate_const(builder, key);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructGetDestructure without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_get_destructure, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, kt, kp, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StructRest without vm pointer".to_string())
                })?;
                let count = exclude_keys.len();
                let (exclude_ptr, count_val) = if count == 0 {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 16) as u32,
                            0,
                        ));
                    for (i, key) in exclude_keys.iter().enumerate() {
                        let (kt, kp) = self.translate_const(builder, key);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(kt, slot, tag_offset);
                        builder.ins().stack_store(kp, slot, payload_offset);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, count as i64);
                    (ptr, cnt)
                };
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.struct_rest, builder.func);
                let call = builder
                    .ins()
                    .call(func_ref, &[st, sp, exclude_ptr, count_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::FirstOrNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.first_or_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::RestOrNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.rest_or_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let idx_val = builder.ins().iconst(I64, *index as i64);
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.array_ref_or_nil, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, idx_val]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Eval { .. } => {
                return Err(JitError::UnsupportedInstruction("Eval".to_string()));
            }

            LirInstr::SuspendingCall {
                dst, func, args, ..
            } => {
                // SuspendingCall: the callee may yield. Handled identically
                // to Call, with an unconditional yield check after the call.
                // LBox cells are first-class values in registers (no auto-
                // unwrap), so yield-through-call spills them correctly.
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("SuspendingCall without vm pointer".to_string())
                })?;

                if args.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let nargs = builder.ins().iconst(I64, 0);
                    let (rt, rp) = self.call_helper_call(
                        builder,
                        ft,
                        fp,
                        null_ptr,
                        nargs,
                        vm,
                        region_id_const,
                    )?;
                    self.def_var_pair(builder, dst.0, rt, rp);
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (args.len() * 16) as u32,
                            0,
                        ));
                    for (i, arg_reg) in args.iter().enumerate() {
                        let (at, ap) = self.use_var_pair(builder, arg_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(at, slot, tag_offset);
                        builder.ins().stack_store(ap, slot, payload_offset);
                    }
                    let args_addr = builder.ins().stack_addr(I64, slot, 0);
                    let nargs = builder.ins().iconst(I64, args.len() as i64);
                    let (rt, rp) = self.call_helper_call(
                        builder,
                        ft,
                        fp,
                        args_addr,
                        nargs,
                        vm,
                        region_id_const,
                    )?;
                    self.def_var_pair(builder, dst.0, rt, rp);
                }
                self.emit_exception_check_after_call(builder)?;
                if self.lir.signal.may_suspend() {
                    let idx = self.call_site_index;
                    self.call_site_index += 1;
                    self.emit_yield_check_after_call(builder, idx)?;
                }
            }

            LirInstr::ArrayMutExtend { dst, array, source } => {
                let (at, ap) = self.use_var_pair(builder, array.0);
                let (srt, srp) = self.use_var_pair(builder, source.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutExtend without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.array_extend,
                    at,
                    ap,
                    srt,
                    srp,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::ArrayMutPush { dst, array, value } => {
                let (at, ap) = self.use_var_pair(builder, array.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("ArrayMutPush without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.array_push,
                    at,
                    ap,
                    vt,
                    vp,
                    vm,
                )?;
                self.emit_exception_check_after_call(builder)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CallArrayMut {
                dst, func, args, ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let (art, arp) = self.use_var_pair(builder, args.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CallArrayMut without vm pointer".to_string())
                })?;
                // call_array: (func_tag, func_payload, arr_tag, arr_payload, vm, region_id)
                let (rt, rp) = self.call_helper_call_array(
                    builder,
                    self.helpers.call_array,
                    ft,
                    fp,
                    art,
                    arp,
                    vm,
                    region_id_const,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
                self.emit_exception_check_after_call(builder)?;
                if self.lir.signal.may_suspend() {
                    let idx = self.call_site_index;
                    self.call_site_index += 1;
                    self.emit_yield_check_after_call(builder, idx)?;
                }
            }

            LirInstr::TailCallArrayMut { func, args, .. } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let (art, arp) = self.use_var_pair(builder, args.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCallArrayMut without vm pointer".to_string())
                })?;
                let (rt, rp) = self.call_helper_call_array(
                    builder,
                    self.helpers.tail_call_array,
                    ft,
                    fp,
                    art,
                    arp,
                    vm,
                    region_id_const,
                )?;
                self.emit_pop_then_return(builder, rt, rp)?;
                return Ok(true);
            }
            _ => return self.translate_instr_predicates(builder, instr),
        }
        Ok(false)
    }
}
