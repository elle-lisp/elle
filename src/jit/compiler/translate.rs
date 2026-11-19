// audited: 2026-11-18
//! The prologue a compiled function runs on its six parameters, then the walk
//! that translates each LIR block and seals it.
//!
//! docs/impl/jit.md
//! docs/impl/region/rules.md

use super::seal::Unsealed;
use super::*;

impl JitCompiler {
    /// Translate LIR function to Cranelift IR
    ///
    /// Each LIR register maps to TWO Cranelift variables: (tag, payload).
    /// The entry block extracts 6 parameters:
    ///   env_ptr, args_ptr, nargs, vm_ptr, self_tag, self_payload
    /// and loads arg Values (16 bytes each) into the doubled arg variables.
    ///
    /// Answers the constant templates the native code bakes pointers into,
    /// which its `JitCode` keeps alive.
    pub(super) fn translate_function(
        &mut self,
        lir: &LirView<'_>,
        _children: ChildTable<'_>,
        func: &mut Function,
    ) -> Result<TranslatedConsts, JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, *lir);

        // Variable layout: each LIR register index `r` maps to TWO Cranelift variables:
        //   tag     at Cranelift var index 2*r
        //   payload at Cranelift var index 2*r+1
        //
        // The "logical" variable space covers:
        //   [0,       num_regs)             - LIR work registers
        //   [num_regs, num_regs+num_locals)  - locals (args + locally-defined)
        // The max logical index is max(num_regs, local_var_base + num_locally_defined).
        // Each logical slot needs 2 Cranelift variables.
        let arg_var_base = lir.num_regs();
        let is_list_variadic = matches!(lir.arity(), Arity::AtLeast(_))
            && matches!(lir.vararg_kind(), crate::hir::VarargKind::List);
        let is_range_arity = matches!(lir.arity(), Arity::Range(_, _));
        let arity_params = if is_list_variadic || is_range_arity {
            lir.num_params() as u16
        } else {
            lir.arity().fixed_params() as u16
        };
        // All num_locals are stack-relative in the dual-address-space lowerer.
        let num_locally_defined = lir.num_locals() as u32;
        let local_var_base = arg_var_base + arity_params as u32;
        let max_logical = std::cmp::max(
            std::cmp::max(
                lir.num_regs() + lir.num_locals() as u32,
                lir.num_locals() as u32,
            ),
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
        for bb in lir.blocks() {
            let cl_block = builder.create_block();
            block_map.insert(bb.label(), cl_block);
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
        // (docs/impl/region/ctx.md). The heap axis grows this slot with a heap
        // capability.
        let jit_ctx_slot =
            builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                std::mem::size_of::<crate::jit::JitCtx>() as u32,
                0,
            ));
        builder.ins().stack_store(I64, vm_ptr, jit_ctx_slot, 0);
        let jit_ctx_ptr = builder.ins().stack_addr(I64, jit_ctx_slot, 0);
        translator.jit_ctx_ptr = Some(jit_ctx_ptr);

        // Push this activation's region-remap frame (the JIT analog of
        // `execute_bytecode_saving_stack`). Emitted before the variadic
        // rest-collection and the body so per-execution alloc regions and
        // slot-resolved `DecrefRegion`s resolve against this frame. Every
        // `return` pops it (via `emit_pop_then_return`), as docs/impl/region/rules.md says.
        translator.emit_push_region_map(&mut builder)?;

        // The arguments, as the interpreter's `populate_env` binds them
        // (src/vm/env.rs). A self tail call binds through the same routine.
        translator.bind_params(&mut builder, args_ptr, nargs, BindSite::Entry)?;

        // Allocate the shared spill slot for the emit, call and charge sites,
        // if there are any. Check the sites directly, not may_suspend(): emit
        // can emit any signal, not just :yield.
        if lir.yield_points().len() != 0
            || lir.call_sites().len() != 0
            || lir.charge_sites().len() != 0
        {
            translator.allocate_shared_spill_slot(&mut builder);
        }

        builder.ins().jump(loop_header, &[]);

        // Loop header: merge point for self-tail-calls. Each trip starts with
        // every local nil, as a fresh activation does, so a release reads nil
        // from a slot this trip never wrote (docs/impl/region/rules.md).
        builder.switch_to_block(loop_header);
        if num_locally_defined > 0 {
            translator.init_locally_defined_vars(&mut builder, num_locally_defined)?;
        }
        let first_lir_block = block_map[&lir.entry()];
        builder.ins().jump(first_lir_block, &[]);

        translator.loop_header = Some(loop_header);

        // Seal each LIR block once every jump into it is in.
        let mut unsealed = Unsealed::new(lir);
        for label in unsealed.ready() {
            builder.seal_block(block_map[&label]);
        }

        // Translate LIR blocks
        for (position, bb) in lir.blocks().enumerate() {
            if self.abandon.is_set() {
                return Err(JitError::Abandoned);
            }
            let cl_block = block_map[&bb.label()];
            builder.switch_to_block(cl_block);
            translator.current_block = position;

            let mut block_terminated = false;
            for (i, node) in bb.nodes().enumerate() {
                if translator.translate_instr(&mut builder, &node.instr(), &block_map)? {
                    translator.skip_unreached_sites(
                        bb.nodes().skip(i + 1).map(|n| n.instr()),
                        &bb.terminator(),
                    )?;
                    block_terminated = true;
                    break;
                }
            }

            if !block_terminated {
                translator.translate_terminator(&mut builder, &bb.terminator(), &block_map)?;
            }
            for label in unsealed.translated(&bb.terminator()) {
                builder.seal_block(block_map[&label]);
            }
        }

        // Metadata/translation agreement: the side-exit helpers index
        // `JitCode.yield_points`, `JitCode.call_sites` and
        // `JitCode.charge_sites` by the translator's running counters, and the
        // emitter recorded those lists in its own walk over the same blocks. A
        // count mismatch means some side-exit would read ANOTHER site's ip and
        // stack shape — a silently corrupted resume. Reject the compile; the
        // interpreter tier is always correct.
        for (what, translated, recorded) in [
            (
                "yield-point",
                translator.yield_point_index,
                lir.yield_points().len(),
            ),
            (
                "call-site",
                translator.call_site_index,
                lir.call_sites().len(),
            ),
            (
                "charge-site",
                translator.charge_site_index,
                lir.charge_sites().len(),
            ),
        ] {
            if translated as usize != recorded {
                return Err(JitError::InvalidLir(format!(
                    "{what} count mismatch: translated {translated}, emitter recorded {recorded}"
                )));
            }
        }

        // The loop header, which every self tail call jumps to.
        builder.seal_all_blocks();
        finalize_function(builder, translator.module);

        Ok(translator.templates)
    }
}
