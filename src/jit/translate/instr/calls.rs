use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Call/TailCall/MakeClosure instructions (chain link from translate_instr).
    pub(super) fn translate_instr_call(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<bool, JitError> {
        match instr {
            LirInstr::Call {
                dst, func, args, ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

                let maybe_scc = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym, peer_func_id)) = maybe_scc {
                    // Call depth check
                    let (overflow_tag, _) =
                        self.call_helper_vm_only(builder, self.helpers.call_depth_enter, vm)?;
                    let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
                    let is_overflow = builder.ins().icmp(IntCC::Equal, overflow_tag, tag_true);
                    let overflow_block = builder.create_block();
                    let call_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_overflow, overflow_block, &[], call_block, &[]);

                    builder.switch_to_block(overflow_block);
                    builder.seal_block(overflow_block);
                    let nil_t = builder.ins().iconst(I64, TAG_NIL as i64);
                    let zero = builder.ins().iconst(I64, 0);
                    self.emit_pop_then_return(builder, nil_t, zero)?;

                    builder.switch_to_block(call_block);
                    builder.seal_block(call_block);

                    let (rt, rp) =
                        self.emit_direct_scc_call(builder, peer_func_id, sym, args, vm)?;
                    self.call_helper_vm_only(builder, self.helpers.call_depth_exit, vm)?;
                    // Resolve pending tail call
                    let func_ref = self
                        .module
                        .declare_func_in_func(self.helpers.resolve_tail_call, builder.func);
                    let call = builder.ins().call(func_ref, &[rt, rp, vm]);
                    let resolved_t = builder.inst_results(call)[0];
                    let resolved_p = builder.inst_results(call)[1];
                    self.def_var_pair(builder, dst.0, resolved_t, resolved_p);
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else if args.is_empty() {
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
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                } else {
                    // Spill args to stack (16 bytes each)
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
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal.may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                }
            }

            LirInstr::TailCall {
                dst, func, args, ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;

                // Self-tail-call optimization
                if let (Some((self_tag, self_payload)), Some(loop_header)) =
                    (self.self_tag_payload, self.loop_header)
                {
                    if args.len() == self.lir.num_params {
                        // Check if func == self (tag AND payload match)
                        let tag_eq = builder.ins().icmp(IntCC::Equal, ft, self_tag);
                        let pay_eq = builder.ins().icmp(IntCC::Equal, fp, self_payload);
                        let is_self = builder.ins().band(tag_eq, pay_eq);

                        let self_call_block = builder.create_block();
                        let other_call_block = builder.create_block();
                        builder
                            .ins()
                            .brif(is_self, self_call_block, &[], other_call_block, &[]);

                        // Self-call path
                        builder.switch_to_block(self_call_block);
                        builder.seal_block(self_call_block);

                        let new_arg_vals: Vec<(
                            cranelift_codegen::ir::Value,
                            cranelift_codegen::ir::Value,
                        )> = args
                            .iter()
                            .map(|arg_reg| self.use_var_pair(builder, arg_reg.0))
                            .collect();

                        // Rotate slab pools: free iteration N-2, preserve N-1
                        // (argument SSA values are in registers, safe from
                        // rotation).
                        //
                        // Only safe when THIS function is rotation-safe: if
                        // its body stores a heap value into external mutable
                        // state (e.g. `(push acc {:index i})` inside the
                        // loop), rotation would dangle the external pointer
                        // exactly like the VM-side trampoline bug fixed via
                        // `body_escapes_heap_values`. When not rotation-safe,
                        // skip the rotation entirely — leak-style correctness
                        // matches the VM interpreter's behavior, which also
                        // declines to rotate non-rotation-safe callers.
                        // rotation_safe removed; skip pool rotation.

                        for (i, (at, ap)) in new_arg_vals.into_iter().enumerate() {
                            let base = self.arg_var_base + i as u32;
                            self.def_var_pair(builder, base, at, ap);
                        }
                        builder.ins().jump(loop_header, &[]);

                        // Other-call path
                        builder.switch_to_block(other_call_block);
                        builder.seal_block(other_call_block);

                        let maybe_scc2 = self
                            .global_load_map
                            .get(func)
                            .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                        if let Some((sym2, peer_func_id)) = maybe_scc2 {
                            // An SCC peer is always a user closure (never a
                            // native), so it always trampolines/returns — no
                            // post-`TailCall` native release to run.
                            let (rt, rp) =
                                self.emit_direct_scc_call(builder, peer_func_id, sym2, args, vm)?;
                            self.emit_pop_then_return(builder, rt, rp)?;
                            return Ok(true);
                        }
                        let (rt, rp) = self.emit_tail_call_with_args(
                            builder,
                            ft,
                            fp,
                            args,
                            vm,
                            region_id_const,
                        )?;
                        // Generic dispatch: a native that completes normally
                        // falls through so the post-`TailCall` releases run
                        // (Inc4 native-tail). Builder is left on the continue
                        // block; keep translating the rest of this LIR block.
                        self.emit_tail_call_result_branch(builder, *dst, rt, rp)?;
                        return Ok(false);
                    }
                }

                // Fallback: no self-tail-call optimization
                let maybe_scc3 = self
                    .global_load_map
                    .get(func)
                    .and_then(|&sym| self.scc_peers.get(&sym).map(|&fid| (sym, fid)));
                if let Some((sym3, peer_func_id)) = maybe_scc3 {
                    // SCC peer → user closure → always trampolines/returns.
                    let (rt, rp) =
                        self.emit_direct_scc_call(builder, peer_func_id, sym3, args, vm)?;
                    self.emit_pop_then_return(builder, rt, rp)?;
                    return Ok(true);
                }

                let (rt, rp) =
                    self.emit_tail_call_with_args(builder, ft, fp, args, vm, region_id_const)?;
                // A native that completes normally falls through to run the
                // post-`TailCall` owned-arg releases; a closure (sentinel),
                // yield, or error returns. Builder is left on the continue
                // block, so keep translating the rest of this LIR block.
                self.emit_tail_call_result_branch(builder, *dst, rt, rp)?;
                return Ok(false);
            }

            LirInstr::MakeClosure {
                dst,
                closure_id,
                captures,
                region,
            } => {
                // Look up the nested LirFunction by ClosureId from module context.
                let func = self
                    .module_closures
                    .get(closure_id.0 as usize)
                    .ok_or_else(|| {
                        JitError::InvalidLir(format!(
                            "MakeClosure: invalid ClosureId({})",
                            closure_id.0
                        ))
                    })?
                    .clone();

                let mut emitter = crate::lir::Emitter::new();

                let lir_module = crate::lir::LirModule {
                    entry: func.clone(),
                    closures: self.module_closures.clone(),
                };
                let (nested_bytecode, nested_yield_points, nested_call_sites) =
                    emitter.emit_module(&lir_module);
                // emit_module returns the entry result; we want the closure's bytecode.
                // Actually, we need to emit just this closure with module context.
                // Use emit_module_closures to get per-closure bytecodes, then index.
                drop(nested_bytecode);
                drop(nested_yield_points);
                drop(nested_call_sites);
                let mut emitter2 = crate::lir::Emitter::new();
                let all_compiled = emitter2.emit_module_closures(&lir_module);
                let (nested_bytecode, nested_yield_points, nested_call_sites) =
                    all_compiled.into_iter().nth(closure_id.0 as usize).unwrap();

                let mut nested_lir = func.clone();
                nested_lir.yield_points = nested_yield_points;
                nested_lir.call_sites = nested_call_sites;

                // The nested lambda's template BLUEPRINT — plain data, owned by
                // the JIT code object (`closure_protos`), NOT a heap `Value`.
                // `elle_jit_make_closure` materializes a FRESH region-allocated
                // `HeapObject::ClosureTemplate` from it per execution (a heap
                // literal is an ordinary, reclaimable allocation). Its own
                // `child_protos` are the nested bytecode's.
                let template = crate::value::TemplateProto {
                    num_locals: func.num_locals as usize,
                    num_captures: captures.len(),
                    num_params: func.num_params,
                    signal: func.signal,
                    capture_params_mask: func.capture_params_mask,
                    capture_locals_mask: func.capture_locals_mask.clone(),
                    location_map: nested_bytecode.location_map,
                    lir_function: Some(std::rc::Rc::new(nested_lir)),
                    doc: func.doc.as_deref().map(str::to_string),
                    syntax: func.syntax.clone(),
                    vararg_kind: func.vararg_kind.clone(),
                    name: func.name.clone(),
                    region_table: func.region_table.clone(),
                    merged_slots: func.merged_slots.iter().map(|s| s.get()).collect(),
                    frame_release_slots: func.frame_release_slots.clone(),
                    frame_release_regions: func
                        .frame_release_regions
                        .iter()
                        .map(|r| r.get())
                        .collect(),
                    child_protos: nested_bytecode.child_protos,
                    ..crate::value::TemplateProto::new(
                        nested_bytecode.instructions,
                        func.arity,
                        nested_bytecode.constants,
                    )
                };

                // Bake a stable raw pointer to the blueprint. The `Rc`'s target
                // address does not move with the vector, and the JIT code object
                // owns this handle for as long as any code it compiled can run.
                self.closure_protos.push(std::rc::Rc::new(template));
                let proto = self.closure_protos.last().expect("just pushed");
                let template_ptr = builder.ins().iconst(I64, std::rc::Rc::as_ptr(proto) as i64);

                let (captures_ptr, count_val) = if captures.is_empty() {
                    (builder.ins().iconst(I64, 0), builder.ins().iconst(I64, 0))
                } else {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            (captures.len() * 16) as u32,
                            0,
                        ));
                    for (i, cap_reg) in captures.iter().enumerate() {
                        let (ct, cp) = self.use_var_pair(builder, cap_reg.0);
                        let tag_offset = (i * 16) as i32;
                        let payload_offset = (i * 16 + 8) as i32;
                        builder.ins().stack_store(ct, slot, tag_offset);
                        builder.ins().stack_store(cp, slot, payload_offset);
                    }
                    let ptr = builder.ins().stack_addr(I64, slot, 0);
                    let cnt = builder.ins().iconst(I64, captures.len() as i64);
                    (ptr, cnt)
                };

                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("MakeClosure without vm pointer".to_string())
                })?;
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.make_closure, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[template_ptr, captures_ptr, count_val, region_val, vm],
                );
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                self.def_var_pair(builder, dst.0, rt, rp);
            }
            _ => return self.translate_instr_async(builder, instr, region_id_const),
        }
        Ok(false)
    }
}
