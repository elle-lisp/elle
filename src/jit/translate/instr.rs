use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Translate a single LIR instruction.
    /// Returns true if the instruction emitted a terminator (e.g., TailCall).
    pub(crate) fn translate_instr(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        _block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<bool, JitError> {
        // The static region slot this instruction is stamped with, read from
        // the variant itself (0 = none). For Call/TailCall this routes the
        // native-result allocation and gates the pass-through retain in
        // `elle_jit_call`, matching the interpreter.
        let region_id_const = builder
            .ins()
            .iconst(I32, instr.region().map_or(0, |r| r.get()) as i64);
        match instr {
            LirInstr::Const { dst, value } => {
                let (tag, payload) = self.translate_const(builder, value);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::ValueConst { dst, value } => {
                let tag = builder.ins().iconst(I64, value.tag as i64);
                let payload = builder.ins().iconst(I64, value.payload as i64);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::MaterializeConst {
                dst,
                template,
                region,
            } => {
                // A heap literal (string, or quoted compound data) is an ordinary
                // allocation, NOT a baked pointer: own the recursive template for
                // the JIT code's lifetime (the native code reads it on every
                // execution to build a FRESH structure), then materialize into
                // this literal's own resolved region — passed explicitly to the
                // helper, exactly like List/MakeArrayMut. The helper recurses in
                // Rust, so one call materializes the whole structure into the
                // resolved region.
                self.templates.push(Box::new(template.clone()));
                let tmpl = self.templates.last().expect("just pushed");
                let ptr = builder
                    .ins()
                    .iconst(I64, (&**tmpl as *const crate::value::ConstTemplate) as i64);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                // A quoted-symbol leaf re-interns into the driving VM's table, so
                // the helper takes the VM pointer.
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("materialize_const without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.materialize_const, builder.func);
                let call = builder.ins().call(func_ref, &[ptr, region_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::LoadLocal { dst, slot } => {
                let base = self.local_slot_to_var(*slot);
                let (tag, payload) = self.use_var_pair(builder, base);
                self.def_var_pair(builder, dst.0, tag, payload);
            }

            LirInstr::StoreLocal { slot, src } => {
                let base = self.local_slot_to_var(*slot);
                let (tag, payload) = self.use_var_pair(builder, src.0);
                self.def_var_pair(builder, base, tag, payload);
            }

            LirInstr::StoreLocalRefcounted { slot, src } => {
                // Refcounting removed — just store (identical to StoreLocal).
                let base = self.local_slot_to_var(*slot);
                let (tag, payload) = self.use_var_pair(builder, src.0);
                self.def_var_pair(builder, base, tag, payload);
            }

            LirInstr::LoadCapture { dst, index } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    // Load from closure environment (captures)
                    // Each Value is 16 bytes: tag at offset i*16, payload at i*16+8
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without env pointer".to_string())
                    })?;
                    let tag_offset = (*index as i32) * 16;
                    let payload_offset = (*index as i32) * 16 + 8;
                    let raw_tag = builder
                        .ins()
                        .load(I64, MemFlags::trusted(), env_ptr, tag_offset);
                    let raw_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), env_ptr, payload_offset);
                    // Auto-unwrap LocalCell if present
                    let (val_tag, val_payload) = self.call_helper_value_unary(
                        builder,
                        self.helpers.load_capture,
                        raw_tag,
                        raw_payload,
                    )?;
                    self.def_var_pair(builder, dst.0, val_tag, val_payload);
                } else if *index < num_captures + arity {
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    if (param_index as u32) < 64
                        && (self.lir.capture_params_mask & (1 << param_index)) != 0
                    {
                        let (rt, rp) = self.call_helper_value_unary(
                            builder,
                            self.helpers.load_capture_cell,
                            tag,
                            payload,
                        )?;
                        self.def_var_pair(builder, dst.0, rt, rp);
                    } else {
                        self.def_var_pair(builder, dst.0, tag, payload);
                    }
                } else {
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    if self.lir.capture_locals_mask.is_set(local_index as usize) {
                        let (rt, rp) = self.call_helper_value_unary(
                            builder,
                            self.helpers.load_capture_cell,
                            tag,
                            payload,
                        )?;
                        self.def_var_pair(builder, dst.0, rt, rp);
                    } else {
                        self.def_var_pair(builder, dst.0, tag, payload);
                    }
                }
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                if *index < num_captures {
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without env pointer".to_string())
                    })?;
                    let tag_offset = (*index as i32) * 16;
                    let payload_offset = (*index as i32) * 16 + 8;
                    let raw_tag = builder
                        .ins()
                        .load(I64, MemFlags::trusted(), env_ptr, tag_offset);
                    let raw_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), env_ptr, payload_offset);
                    self.def_var_pair(builder, dst.0, raw_tag, raw_payload);
                } else if *index < num_captures + arity {
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    self.def_var_pair(builder, dst.0, tag, payload);
                } else {
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    let (tag, payload) = self.use_var_pair(builder, base);
                    self.def_var_pair(builder, dst.0, tag, payload);
                }
            }

            LirInstr::LoadSelf { dst } => {
                // The executing closure is passed to every compiled body as the
                // (self_tag, self_payload) parameter pair (`self_tag_payload`,
                // also the self-tail-call target), so the value path reads it
                // directly — no capture slot, no runtime-register load.
                let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
                    JitError::InvalidLir("LoadSelf without self_tag_payload".to_string())
                })?;
                self.def_var_pair(builder, dst.0, self_tag, self_payload);
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let (lt, lp) = self.use_var_pair(builder, lhs.0);
                let (rt, rp) = self.use_var_pair(builder, rhs.0);
                let (rt2, rp2) = self.call_binary_helper(builder, *op, lt, lp, rt, rp)?;
                self.def_var_pair(builder, dst.0, rt2, rp2);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) = self.call_unary_helper(builder, *op, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                let (lt, lp) = self.use_var_pair(builder, lhs.0);
                let (rt, rp) = self.use_var_pair(builder, rhs.0);
                let (crt, crp) = self.call_compare_helper(builder, *op, lt, lp, rt, rp)?;
                self.def_var_pair(builder, dst.0, crt, crp);
            }

            LirInstr::Convert { dst, op, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let func_id = match op {
                    crate::lir::ConvOp::IntToFloat => self.helpers.int_to_float,
                    crate::lir::ConvOp::FloatToInt => self.helpers.float_to_int,
                };
                let func_ref = self.module.declare_func_in_func(func_id, builder.func);
                let call = builder.ins().call(func_ref, &[st, sp]);
                let results = builder.inst_results(call);
                let rt = results[0];
                let rp = results[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsNil { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_nil, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::IsPair { dst, src } => {
                let (st, sp) = self.use_var_pair(builder, src.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.is_pair, st, sp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::List {
                dst,
                head,
                tail,
                region,
            } => {
                let (ht, hp) = self.use_var_pair(builder, head.0);
                let (tt, tp) = self.use_var_pair(builder, tail.0);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("List without vm pointer".to_string()))?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.pair, builder.func);
                let call = builder
                    .ins()
                    .call(func_ref, &[ht, hp, tt, tp, region_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::First { dst, pair } => {
                let (pt, pp) = self.use_var_pair(builder, pair.0);
                let (rt, rp) = self.call_helper_value_unary(builder, self.helpers.first, pt, pp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::Rest { dst, pair } => {
                let (pt, pp) = self.use_var_pair(builder, pair.0);
                let (rt, rp) = self.call_helper_value_unary(builder, self.helpers.rest, pt, pp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::MakeArrayMut {
                dst,
                elements,
                region,
            } => {
                if elements.is_empty() {
                    let null_ptr = builder.ins().iconst(I64, 0);
                    let count = builder.ins().iconst(I64, 0);
                    let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                    let vm = self.vm_ptr.ok_or_else(|| {
                        JitError::InvalidLir("MakeArrayMut without vm pointer".to_string())
                    })?;
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.make_array, builder.func);
                    let call = builder
                        .ins()
                        .call(func_ref, &[null_ptr, count, region_val, vm]);
                    let rt = builder.inst_results(call)[0];
                    let rp = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, rt, rp);
                } else {
                    // Each Value is 16 bytes on the stack
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (elements.len() * 16) as u32,
                            0,
                        ));
                    for (i, elem_reg) in elements.iter().enumerate() {
                        let (et, ep) = self.use_var_pair(builder, elem_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(et, slot, tag_offset);
                        builder.ins().stack_store(ep, slot, payload_offset);
                    }
                    let elements_addr = builder.ins().stack_addr(I64, slot, 0);
                    let count = builder.ins().iconst(I64, elements.len() as i64);
                    let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                    let vm = self.vm_ptr.ok_or_else(|| {
                        JitError::InvalidLir("MakeArrayMut without vm pointer".to_string())
                    })?;
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.make_array, builder.func);
                    let call = builder
                        .ins()
                        .call(func_ref, &[elements_addr, count, region_val, vm]);
                    let rt = builder.inst_results(call)[0];
                    let rp = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, rt, rp);
                }
            }

            LirInstr::MakeCaptureCell {
                dst, value, region, ..
            } => {
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("MakeCaptureCell without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.make_capture, builder.func);
                let call = builder.ins().call(func_ref, &[vt, vp, region_val, vm]);
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::LoadCaptureCell { dst, cell } => {
                let (ct, cp) = self.use_var_pair(builder, cell.0);
                let (rt, rp) =
                    self.call_helper_value_unary(builder, self.helpers.load_capture_cell, ct, cp)?;
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            LirInstr::StoreCaptureCell { cell, value } => {
                let (ct, cp) = self.use_var_pair(builder, cell.0);
                let (vt, vp) = self.use_var_pair(builder, value.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StoreCaptureCell without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.store_capture_cell, builder.func);
                let call = builder.ins().call(func_ref, &[ct, cp, vt, vp, vm]);
                let _ = builder.inst_results(call);
            }

            LirInstr::StoreCapture { index, src } => {
                let num_captures = self.lir.num_captures;
                let arity = self.lir.num_params as u16;
                let (vt, vp) = self.use_var_pair(builder, src.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("StoreCapture without vm pointer".to_string())
                })?;

                if *index < num_captures {
                    // Store to a capture slot in the closure env
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("StoreCapture without env pointer".to_string())
                    })?;
                    let idx_val = builder.ins().iconst(I64, *index as i64);
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.store_capture, builder.func);
                    let call = builder
                        .ins()
                        .call(func_ref, &[env_ptr, idx_val, vt, vp, vm]);
                    let _ = builder.inst_results(call);
                } else if *index < num_captures + arity {
                    let param_index = *index - num_captures;
                    let base = self.arg_var_base + param_index as u32;
                    if (param_index as u32) < 64
                        && (self.lir.capture_params_mask & (1 << param_index)) != 0
                    {
                        let (ct, cp) = self.use_var_pair(builder, base);
                        let func_ref = self
                            .module
                            .declare_func_in_func(self.helpers.store_capture_cell, builder.func);
                        let call = builder.ins().call(func_ref, &[ct, cp, vt, vp, vm]);
                        let _ = builder.inst_results(call);
                    } else {
                        self.def_var_pair(builder, base, vt, vp);
                    }
                } else {
                    let local_index = *index - num_captures - arity;
                    let jit_slot = self.lir.num_local_params as u32 + local_index as u32;
                    let base = self.local_var_base + jit_slot;
                    if self.lir.capture_locals_mask.is_set(local_index as usize) {
                        let (ct, cp) = self.use_var_pair(builder, base);
                        let func_ref = self
                            .module
                            .declare_func_in_func(self.helpers.store_capture_cell, builder.func);
                        let call = builder.ins().call(func_ref, &[ct, cp, vt, vp, vm]);
                        let _ = builder.inst_results(call);
                    } else {
                        self.def_var_pair(builder, base, vt, vp);
                    }
                }
            }
            _ => return self.translate_instr_call(builder, instr, region_id_const),
        }
        Ok(false)
    }
}

mod async_ops;
mod calls;
mod predicates;
