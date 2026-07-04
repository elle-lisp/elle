use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Region-refcount, predicate/type-check, and collection-op instructions (chain tail).
    pub(super) fn translate_instr_predicates(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::IncrefRegion { region_id } => {
                // Resolve the static slot through THIS activation's region map
                // (in the helper), not as a physical region id. Mirror of the
                // interpreter's defensive `IncrefRegion` arm.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("IncrefRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.incref_region, builder.func);
                let rid = builder.ins().iconst(I32, region_id.get() as i64);
                builder.ins().call(func_ref, &[vm, rid]);
            }

            LirInstr::DecrefRegion { region_id } => {
                // Resolve+clear the static slot through the activation map (in the
                // helper via `take_runtime_region_for_drop_slot`) and decref the
                // physical region — never treat the slot id as a physical region.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_region, builder.func);
                let rid = builder.ins().iconst(I32, region_id.get() as i64);
                builder.ins().call(func_ref, &[vm, rid]);
            }

            LirInstr::DecrefValueRegion { src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefValueRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_value_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::DecrefCellRegion { src } => {
                // Free the CELL's own region via `region_of` (NOT
                // `result_region_of`): `elle_jit_decref_cell_region`, mirroring
                // the interpreter's `DecrefCellRegion` arm. `DecrefValueRegion`
                // (below/above) uses `result_region_of` to unwrap a capture cell
                // to the inner value — the two must not be conflated, or a
                // cell-wrapped call result double-frees (the redis eager-JIT
                // crash). Inc6 region_of/result_region_of reconciliation.
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("DecrefCellRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.decref_cell_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::IncrefValueRegion { src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("IncrefValueRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.incref_value_region, builder.func);
                builder.ins().call(func_ref, &[st, sp, vm]);
            }

            LirInstr::AdoptRegion { parent, child } => {
                // Link the child's region as Owned by the parent's region — the
                // runtime `AdoptRegion` (docs/impl/region-model.md § "Adoption and
                // subtree drop"). Value-resolved like `IncrefValueRegion`/
                // `DecrefValueRegion`: load both values and hand them to the
                // helper, which resolves each to its runtime region and adopts.
                // Mirrors the interpreter's `handle_adopt_region`.
                let (pt, pp) = self.use_var_pair(builder, parent.0);
                let (ct, cp) = self.use_var_pair(builder, child.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("AdoptRegion without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.adopt_region, builder.func);
                let call = builder.ins().call(func_ref, &[pt, pp, ct, cp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::AdoptIntoActivation { child } => {
                // Adopt the child's region into the current activation's owner
                // node — the runtime channel of the activation-ownership cuts
                // (docs/impl/region-model.md § "Owner nodes"). Value-resolved
                // like `AdoptRegion`, with no parent operand (the node is VM
                // state, minted lazily by the helper). Mirrors the
                // interpreter's `handle_adopt_into_activation`.
                let (ct, cp) = self.use_var_pair(builder, child.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("AdoptIntoActivation without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.adopt_into_activation, builder.func);
                let call = builder.ins().call(func_ref, &[ct, cp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::FreeRegionGroup { members } => {
                // Free a co-owned region group as one unit — the runtime
                // `FreeRegionGroup`. Spill each member value to a stack slot
                // (16 bytes: tag, payload) and pass a pointer + count to the
                // helper (exactly as `PushParamFrame` spills its pairs), which
                // resolves each to its runtime region and frees the whole set.
                // Mirrors the interpreter's `handle_free_region_group`.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("FreeRegionGroup without vm pointer".to_string())
                })?;
                let count = members.len();
                let (members_ptr, count_val) = if count == 0 {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (count * 16) as u32,
                            0,
                        ));
                    for (i, member_reg) in members.iter().enumerate() {
                        let (mt, mp) = self.use_var_pair(builder, member_reg.0);
                        let base = (i * 16) as i32;
                        builder.ins().stack_store(mt, slot, base);
                        builder.ins().stack_store(mp, slot, base + 8);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, count as i64);
                    (ptr, cnt)
                };
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.free_region_group, builder.func);
                let call = builder.ins().call(func_ref, &[members_ptr, count_val, vm]);
                let _ = builder.inst_results(call);
            }

            // The coalescing equivalence oracle is a VM-interp-only debug
            // instrument; the JIT translates it to nothing. Coalesced sites on
            // the optimizing tiers are covered by cross-tier divergence + the
            // escape golden (docs/impl/region-rules.md § "the equivalence oracle").
            LirInstr::AssertRegionMatches { .. } => {}

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

            // Flip rotation is VM-only; the JIT uses `rotate_pools_jit`
            // via its own trampoline path.
            // === New intrinsic type predicates ===
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
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.length, st, sp)?;
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
            _ => unreachable!("translate_instr_predicates: instruction handled earlier in chain"),
        }
        Ok(false)
    }
}
