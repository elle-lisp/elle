// audited: 2026-09-29
//! Translating parameter frames, signal bounds, type predicates, and the data-access and mutability intrinsics.
//!
//! src/jit/AGENTS.md

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Parameter-frame, signal-bound, predicate/type-check and collection-op
    /// instructions. The region instructions continue the chain.
    pub(super) fn translate_instr_predicates(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::PushParamFrame { pairs } => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PushParamFrame without vm pointer".to_string())
                })?;
                let count = pairs.len();
                if count == 0 {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count_val = builder.ins().iconst(I64, 0);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.push_param_frame, builder.func);
                    let call = builder.ins().call(func_ref, &[null_ptr, count_val, vm]);
                    let _ = builder.inst_results(call);
                } else {
                    // Spill pairs as Values (16 bytes each): [param0, val0, param1, val1, ...]
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 2 * 16) as u32,
                            0,
                        ));
                    for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                        let (pt, pp) = self.use_var_pair(builder, param_reg.0);
                        let (vt, vp) = self.use_var_pair(builder, val_reg.0);
                        let base = i * 2 * 16;
                        builder.ins().stack_store(pt, slot, base as i32);
                        builder.ins().stack_store(pp, slot, (base + 8) as i32);
                        builder.ins().stack_store(vt, slot, (base + 16) as i32);
                        builder.ins().stack_store(vp, slot, (base + 24) as i32);
                    }
                    let pairs_ptr = builder.ins().stack_addr(I64, slot, 0);
                    let count_val = builder.ins().iconst(I64, count as i64);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.push_param_frame, builder.func);
                    let call = builder.ins().call(func_ref, &[pairs_ptr, count_val, vm]);
                    let _ = builder.inst_results(call);
                }
                self.emit_exception_check_after_call(builder)?;
            }

            LirInstr::PopParamFrame => {
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("PopParamFrame without vm pointer".to_string())
                })?;
                self.call_helper_vm_only(builder, self.helpers.pop_param_frame, vm)?;
            }

            LirInstr::IsSet { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_set, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsSetMut { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_set_mut, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::CheckSignalBound { src, allowed_bits } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let allowed_val = builder.ins().iconst(I64, allowed_bits.raw() as i64);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("CheckSignalBound without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.check_signal_bound, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, allowed_val, vm]);
                let _ = builder.inst_results(call);
                self.emit_exception_check_after_call(builder)?;
            }

            // === Intrinsic type predicates ===
            LirInstr::IsEmpty { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_empty, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsBool { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_bool, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsInt { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_int, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsFloat { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_float, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsString { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_string, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsKeyword { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_keyword, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsSymbolCheck { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_symbol_check, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsBytes { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_bytes, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsBox { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_box, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsClosure { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_closure, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IsFiber { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_fiber, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::TypeOf { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.type_of, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            // === Data access ===
            LirInstr::Length { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                // `%length` segments string arms under the VM's Unicode
                // generation, reached through the threaded `JitCtx` (passed in
                // the vm pointer slot of the `value_unary_vm` ABI, like `%pop`).
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.length, st, sp, jit_ctx)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Get { dst, obj, key } => {
                let (ot, op) = self.use_var_pair(builder, obj.0);
                let (kt, kp) = self.use_var_pair(builder, key.0);
                let (rt, rp) =
                    self.call_helper_value_binary(builder, self.helpers.get, ot, op, kt, kp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Put { dst, obj, key, val } => {
                let (ot, op) = self.use_var_pair(builder, obj.0);
                let (kt, kp) = self.use_var_pair(builder, key.0);
                let (vt, vp) = self.use_var_pair(builder, val.0);
                // Trailing arg is this activation's `JitCtx`; the helper resolves
                // its VM from it.
                let jit_ctx = self.jit_ctx()?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.put, builder.func);
                let call = builder
                    .ins()
                    .call(func_ref, &[ot, op, kt, kp, vt, vp, jit_ctx]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Del { dst, obj, key } => {
                let (ot, op) = self.use_var_pair(builder, obj.0);
                let (kt, kp) = self.use_var_pair(builder, key.0);
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.del,
                    ot,
                    op,
                    kt,
                    kp,
                    jit_ctx,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Has { dst, obj, key } => {
                let (ot, op) = self.use_var_pair(builder, obj.0);
                let (kt, kp) = self.use_var_pair(builder, key.0);
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.has,
                    ot,
                    op,
                    kt,
                    kp,
                    jit_ctx,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IntrPush { dst, array, value } => {
                let (at, ap) = self.use_var_pair(builder, array.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.intr_push,
                    at,
                    ap,
                    vt,
                    vp,
                    jit_ctx,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IntrStringPush { dst, string, value } => {
                let (st, sp) = self.use_var_pair(builder, string.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.intr_string_push,
                    st,
                    sp,
                    vt,
                    vp,
                    jit_ctx,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::IntrBytesPush { dst, bytes, value } => {
                let (bt, bp) = self.use_var_pair(builder, bytes.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) = self.call_helper_value_binary_vm(
                    builder,
                    self.helpers.intr_bytes_push,
                    bt,
                    bp,
                    vt,
                    vp,
                    jit_ctx,
                )?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Pop { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                // `%pop` decrefs the popped value's region on the instance's own
                // heap, reached through the threaded `JitCtx` (passed in the vm
                // pointer slot of the `value_unary_vm` ABI).
                let jit_ctx = self.jit_ctx()?;
                let (rt, rp) =
                    self.call_helper_value_vm(builder, self.helpers.pop, st, sp, jit_ctx)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            // === Mutability ===
            // %freeze / %thaw are `IntrinsicOp::allocates` ops: the lowerer's
            // emit_alloc stamps each with a static region SLOT and a matching
            // `DecrefRegion(slot)`. Resolve the slot to its physical region id
            // (the same region-id ABI `List`/`MakeArrayMut` use) and thread it
            // to the helper so the fresh copy is born in that region. Mirrors the
            // interpreter's `runtime_region_for_alloc_slot` +
            // `handle_intr_freeze/thaw(region)`.
            LirInstr::Freeze { dst, src, region } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let jit_ctx = self.jit_ctx()?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.freeze, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, region_val, jit_ctx]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            LirInstr::Thaw { dst, src, region } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let jit_ctx = self.jit_ctx()?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.thaw, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp, region_val, jit_ctx]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            // === Identity ===
            LirInstr::Identical { dst, lhs, rhs } => {
                let (lt, lp) = self.use_var_pair(builder, lhs.0);
                let (rt, rp) = self.use_var_pair(builder, rhs.0);
                let (crt, crp) =
                    self.call_helper_value_binary(builder, self.helpers.identical, lt, lp, rt, rp)?;
                self.def_var_pair(builder, dst.0, crt, crp);
            }
            _ => return self.translate_instr_region(builder, instr),
        }
        Ok(false)
    }
}
