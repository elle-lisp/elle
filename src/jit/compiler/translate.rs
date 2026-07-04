use super::*;

impl JitCompiler {
    /// Translate LIR function to Cranelift IR
    ///
    /// Each LIR register maps to TWO Cranelift variables: (tag, payload).
    /// The entry block extracts 6 parameters:
    ///   env_ptr, args_ptr, nargs, vm_ptr, self_tag, self_payload
    /// and loads arg Values (16 bytes each) into the doubled arg variables.
    pub(super) fn translate_function(
        &mut self,
        lir: &LirFunction,
        func: &mut Function,
        scc_peers: Option<&HashMap<SymbolId, FuncId>>,
        self_sym: Option<SymbolId>,
        symbol_names: HashMap<u32, String>,
        module_closures: Vec<LirFunction>,
    ) -> Result<TranslatedConsts, JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, lir);
        translator.symbol_names = symbol_names;
        translator.module_closures = module_closures;

        translator.self_sym = self_sym;

        if let Some(peers) = scc_peers {
            translator.scc_peers = peers.clone();
        }

        // Variable layout: each LIR register index `r` maps to TWO Cranelift variables:
        //   tag     at Cranelift var index 2*r
        //   payload at Cranelift var index 2*r+1
        //
        // The "logical" variable space covers:
        //   [0,       num_regs)             - LIR work registers
        //   [num_regs, num_regs+num_locals)  - locals (args + locally-defined)
        // The max logical index is max(num_regs, local_var_base + num_locally_defined).
        // Each logical slot needs 2 Cranelift variables.
        let arg_var_base = lir.num_regs;
        let is_list_variadic = matches!(lir.arity, Arity::AtLeast(_))
            && matches!(lir.vararg_kind, crate::hir::VarargKind::List);
        let is_range_arity = matches!(lir.arity, Arity::Range(_, _));
        let arity_params = if is_list_variadic || is_range_arity {
            lir.num_params as u16
        } else {
            lir.arity.fixed_params() as u16
        };
        // All num_locals are stack-relative in the dual-address-space lowerer.
        let num_locally_defined = lir.num_locals as u32;
        let local_var_base = arg_var_base + arity_params as u32;
        let max_logical = std::cmp::max(
            std::cmp::max(lir.num_regs + lir.num_locals as u32, lir.num_locals as u32),
            local_var_base + num_locally_defined,
        );
        // Declare 2 * max_logical Cranelift variables (tag + payload per slot).
        // declare_var allocates sequentially from 0, so the returned Variable
        // indices match our var(i) scheme.
        for _ in 0..(2 * max_logical) {
            builder.declare_var(I64);
        }
        translator.arg_var_base = arg_var_base;
        translator.local_var_base = local_var_base;

        // Create blocks
        let entry_block = builder.create_block();
        let loop_header = builder.create_block();

        let mut block_map: HashMap<Label, cranelift_codegen::ir::Block> = HashMap::new();
        for bb in &lir.blocks {
            let cl_block = builder.create_block();
            block_map.insert(bb.label, cl_block);
        }

        // Entry block: extract 6 function parameters
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let env_ptr = builder.block_params(entry_block)[0];
        let args_ptr = builder.block_params(entry_block)[1];
        let nargs = builder.block_params(entry_block)[2];
        let vm_ptr = builder.block_params(entry_block)[3];
        let self_tag = builder.block_params(entry_block)[4];
        let self_payload = builder.block_params(entry_block)[5];

        translator.env_ptr = Some(env_ptr);
        translator.vm_ptr = Some(vm_ptr);
        translator.self_tag_payload = Some((self_tag, self_payload));

        // Build this activation's `JitCtx` capability bundle: a stack slot holding
        // the driving VM pointer (offset 0, matching `JitCtx`'s `#[repr(C)]`
        // layout). Its address is threaded to the intrinsic fast-path helpers so
        // they resolve the VM from the bundle, keeping the VM dependency explicit so
        // two embedded instances on one thread each reach their own VM
        // (docs/impl/region-ctx.md "JIT intrinsic helpers reach the VM through a
        // JitCtx"). The heap axis grows this slot with a heap capability.
        let jit_ctx_slot =
            builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                std::mem::size_of::<crate::jit::JitCtx>() as u32,
                0,
            ));
        builder.ins().stack_store(vm_ptr, jit_ctx_slot, 0);
        let jit_ctx_ptr = builder.ins().stack_addr(I64, jit_ctx_slot, 0);
        translator.jit_ctx_ptr = Some(jit_ctx_ptr);

        // Push this activation's region-remap frame (the JIT analog of
        // `execute_bytecode_saving_stack`). Emitted before the variadic
        // rest-collection and the body so per-execution alloc regions and
        // slot-resolved `DecrefRegion`s resolve against this frame. Every
        // `return` pops it (via `emit_pop_then_return`). docs/impl/region-rules.md Rule 4.
        translator.emit_push_region_map(&mut builder)?;

        if is_list_variadic {
            // --- Variadic entry: load required+optional params, then cons list for rest ---
            let required = lir.arity.fixed_params();
            // num_params includes the rest param slot — subtract 1 for non-rest count
            let non_rest_params = lir.num_params.saturating_sub(1);
            let has_opt_params = non_rest_params > required;

            // Load required params unconditionally
            for i in 0..required as u32 {
                let tag_offset = (i as i32) * 16;
                let payload_offset = (i as i32) * 16 + 8;
                let arg_tag = builder
                    .ins()
                    .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                let arg_payload =
                    builder
                        .ins()
                        .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                let base = arg_var_base + i;
                if (i as u64) < 64 && (lir.capture_params_mask & (1 << i)) != 0 {
                    let (cell_t, cell_p) = translator.call_helper_value_vm(
                        &mut builder,
                        translator.helpers.make_capture_owned,
                        arg_tag,
                        arg_payload,
                        vm_ptr,
                    )?;
                    translator.def_var_pair(&mut builder, base, cell_t, cell_p);
                } else {
                    translator.def_var_pair(&mut builder, base, arg_tag, arg_payload);
                }
            }

            // Load optional params conditionally (check nargs for each)
            if has_opt_params {
                for i in required as u32..non_rest_params as u32 {
                    let base = arg_var_base + i;
                    let threshold = builder.ins().iconst(I64, i as i64 + 1);
                    let has_arg =
                        builder
                            .ins()
                            .icmp(IntCC::UnsignedGreaterThanOrEqual, nargs, threshold);
                    let then_block = builder.create_block();
                    let else_block = builder.create_block();
                    let merge_block = builder.create_block();
                    builder.append_block_param(merge_block, I64);
                    builder.append_block_param(merge_block, I64);

                    builder
                        .ins()
                        .brif(has_arg, then_block, &[], else_block, &[]);

                    builder.switch_to_block(then_block);
                    builder.seal_block(then_block);
                    let tag_offset = (i as i32) * 16;
                    let payload_offset = (i as i32) * 16 + 8;
                    let arg_tag =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                    let arg_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                    builder.ins().jump(
                        merge_block,
                        &[BlockArg::Value(arg_tag), BlockArg::Value(arg_payload)],
                    );

                    builder.switch_to_block(else_block);
                    builder.seal_block(else_block);
                    let nil_tag = builder
                        .ins()
                        .iconst(I64, crate::value::Value::NIL.tag as i64);
                    let nil_pay = builder.ins().iconst(I64, 0);
                    builder.ins().jump(
                        merge_block,
                        &[BlockArg::Value(nil_tag), BlockArg::Value(nil_pay)],
                    );

                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    let merged_tag = builder.block_params(merge_block)[0];
                    let merged_pay = builder.block_params(merge_block)[1];

                    if (i as u64) < 64 && (lir.capture_params_mask & (1 << i)) != 0 {
                        let (cell_t, cell_p) = translator.call_helper_value_vm(
                            &mut builder,
                            translator.helpers.make_capture_owned,
                            merged_tag,
                            merged_pay,
                            vm_ptr,
                        )?;
                        translator.def_var_pair(&mut builder, base, cell_t, cell_p);
                    } else {
                        translator.def_var_pair(&mut builder, base, merged_tag, merged_pay);
                    }
                }
            }

            // Collect args[non_rest_params..nargs] into the rest list. Each cons
            // is minted in its OWN fresh per-execution region with ownership
            // transfer down the chain — `elle_jit_collect_rest_list`, the JIT
            // analog of the interpreter's `args_to_list` (src/vm/env.rs). Each
            // cons owning its own per-execution region keeps the rest list's
            // regions independent of a JIT->JIT callee's. docs/impl/region-rules.md.
            let rest_var_idx = arg_var_base + non_rest_params as u32;
            let start_const = builder.ins().iconst(I32, non_rest_params as i64);
            let nargs_i32 = builder.ins().ireduce(I32, nargs);
            let rest_ref = translator
                .module
                .declare_func_in_func(translator.helpers.collect_rest_list, builder.func);
            let rest_call = builder
                .ins()
                .call(rest_ref, &[args_ptr, start_const, nargs_i32, vm_ptr]);
            let rest_tag = builder.inst_results(rest_call)[0];
            let rest_payload = builder.inst_results(rest_call)[1];

            // Handle capture_params_mask for the rest param
            let rest_param_index = non_rest_params;
            if rest_param_index < 64 && (lir.capture_params_mask & (1 << rest_param_index)) != 0 {
                let (cell_t, cell_p) = translator.call_helper_value_vm(
                    &mut builder,
                    translator.helpers.make_capture_owned,
                    rest_tag,
                    rest_payload,
                    vm_ptr,
                )?;
                translator.def_var_pair(&mut builder, rest_var_idx, cell_t, cell_p);
            } else {
                translator.def_var_pair(&mut builder, rest_var_idx, rest_tag, rest_payload);
            }

            // NOTE: cons_loop_head is NOT sealed here — sealed by seal_all_blocks() below.
        } else {
            // --- Non-variadic entry: load args directly (16 bytes each) ---
            let required = lir.arity.fixed_params() as u32;
            for i in 0..arity_params as u32 {
                let base = arg_var_base + i;
                let is_optional = is_range_arity && i >= required;

                if is_optional {
                    // Optional param: check nargs > i, load arg or default to nil
                    let threshold = builder.ins().iconst(I64, i as i64 + 1);
                    let has_arg =
                        builder
                            .ins()
                            .icmp(IntCC::UnsignedGreaterThanOrEqual, nargs, threshold);
                    let then_block = builder.create_block();
                    let else_block = builder.create_block();
                    let merge_block = builder.create_block();
                    builder.append_block_param(merge_block, I64); // tag
                    builder.append_block_param(merge_block, I64); // payload

                    builder
                        .ins()
                        .brif(has_arg, then_block, &[], else_block, &[]);

                    // then: load from args
                    builder.switch_to_block(then_block);
                    builder.seal_block(then_block);
                    let tag_offset = (i as i32) * 16;
                    let payload_offset = (i as i32) * 16 + 8;
                    let arg_tag =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                    let arg_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                    builder.ins().jump(
                        merge_block,
                        &[BlockArg::Value(arg_tag), BlockArg::Value(arg_payload)],
                    );

                    // else: nil
                    builder.switch_to_block(else_block);
                    builder.seal_block(else_block);
                    let nil_tag = builder
                        .ins()
                        .iconst(I64, crate::value::Value::NIL.tag as i64);
                    let nil_pay = builder.ins().iconst(I64, 0);
                    builder.ins().jump(
                        merge_block,
                        &[BlockArg::Value(nil_tag), BlockArg::Value(nil_pay)],
                    );

                    // merge
                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    let merged_tag = builder.block_params(merge_block)[0];
                    let merged_pay = builder.block_params(merge_block)[1];

                    if (i as u64) < 64 && (lir.capture_params_mask & (1 << i)) != 0 {
                        let (cell_t, cell_p) = translator.call_helper_value_vm(
                            &mut builder,
                            translator.helpers.make_capture_owned,
                            merged_tag,
                            merged_pay,
                            vm_ptr,
                        )?;
                        translator.def_var_pair(&mut builder, base, cell_t, cell_p);
                    } else {
                        translator.def_var_pair(&mut builder, base, merged_tag, merged_pay);
                    }
                } else {
                    // Required param: load unconditionally
                    let tag_offset = (i as i32) * 16;
                    let payload_offset = (i as i32) * 16 + 8;
                    let arg_tag =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, tag_offset);
                    let arg_payload =
                        builder
                            .ins()
                            .load(I64, MemFlags::trusted(), args_ptr, payload_offset);
                    if (i as u64) < 64 && (lir.capture_params_mask & (1 << i)) != 0 {
                        let (cell_t, cell_p) = translator.call_helper_value_vm(
                            &mut builder,
                            translator.helpers.make_capture_owned,
                            arg_tag,
                            arg_payload,
                            vm_ptr,
                        )?;
                        translator.def_var_pair(&mut builder, base, cell_t, cell_p);
                    } else {
                        translator.def_var_pair(&mut builder, base, arg_tag, arg_payload);
                    }
                }
            }
        }

        // Initialize locally-defined variables
        if num_locally_defined > 0 {
            translator.init_locally_defined_vars(&mut builder, num_locally_defined)?;
        }

        // Allocate shared spill slot for emit/call sites (if any).
        // Check yield_points (emit terminators) and call_sites directly,
        // not may_suspend() — emit can emit any signal, not just :yield.
        if !lir.yield_points.is_empty() || !lir.call_sites.is_empty() {
            translator.allocate_shared_spill_slot(&mut builder);
        }

        builder.ins().jump(loop_header, &[]);

        // Loop header: merge point for self-tail-calls
        builder.switch_to_block(loop_header);
        let first_lir_block = block_map[&lir.entry];
        builder.ins().jump(first_lir_block, &[]);

        translator.loop_header = Some(loop_header);

        // Translate LIR blocks
        for bb in &lir.blocks {
            let cl_block = block_map[&bb.label];
            builder.switch_to_block(cl_block);

            let mut block_terminated = false;
            for spanned in &bb.instructions {
                if translator.translate_instr(&mut builder, &spanned.instr, &block_map)? {
                    block_terminated = true;
                    break;
                }
            }

            if !block_terminated {
                translator.translate_terminator(
                    &mut builder,
                    &bb.terminator.terminator,
                    &block_map,
                )?;
            }
        }

        builder.seal_all_blocks();
        builder.finalize();

        Ok((translator.closure_protos, translator.templates))
    }
}
