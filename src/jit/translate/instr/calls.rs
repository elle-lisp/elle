// audited: 2026-10-06
// docs/impl/jit.md
//! How a call leaves compiled code: the self-tail-call loop, the helper that
//! carries every other call, and the `MakeClosure` refusal.

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Call/TailCall/MakeClosure instructions (chain link from translate_instr).
    /// A `MakeClosure` is refused as unsupported.
    pub(super) fn translate_instr_call(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &InstrRef<'_>,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<bool, JitError> {
        match instr {
            InstrRef::Call {
                dst, func, args, ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

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
                    self.emit_exception_check_after_call(builder)?;
                    if self.lir.signal().may_suspend() {
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
                        store_value_slot(builder, slot, i as u32, at, ap);
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
                    if self.lir.signal().may_suspend() {
                        let idx = self.call_site_index;
                        self.call_site_index += 1;
                        self.emit_yield_check_after_call(builder, idx)?;
                    }
                }
            }

            InstrRef::TailCall {
                dst,
                func,
                args,
                defer_callee_release,
                deferred_release_slot,
                ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;
                // The releases this call strands past the frame replacement. The
                // helper hands them to the activation that runs the callee, this
                // one having popped its own dues slot by then
                // (docs/impl/region/relocate.md § "A channel built in compiled
                // code hands its release forward").
                let defer = TailDeferrals::of(*defer_callee_release, *deferred_release_slot);

                // The emitter records one call site per tail call of a function
                // that may suspend; take it before the self-call branch so the
                // counts agree whichever path runs.
                let park_site = if self.lir.signal().may_suspend() {
                    let idx = self.call_site_index;
                    self.call_site_index += 1;
                    Some(idx)
                } else {
                    None
                };

                // Self-tail-call optimization
                if let (Some((self_tag, self_payload)), Some(loop_header)) =
                    (self.self_tag_payload, self.loop_header)
                {
                    if args.len() == self.lir.num_params() {
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

                        // Every new argument is read above, before any is
                        // written, so `(f b a)` swaps rather than clobbers. The
                        // loop replaces no frame and stays in this activation, so
                        // it hands `defer` nowhere: the callee IS this function,
                        // and the release the deferral would supply is its own.
                        for (i, (at, ap)) in new_arg_vals.into_iter().enumerate() {
                            let base = self.arg_var_base + i as u32;
                            self.def_var_pair(builder, base, at, ap);
                        }
                        builder.ins().jump(loop_header, &[]);

                        // Other-call path
                        builder.switch_to_block(other_call_block);
                        builder.seal_block(other_call_block);

                        let (rt, rp) = self.emit_tail_call_with_args(
                            builder,
                            ft,
                            fp,
                            args,
                            vm,
                            region_id_const,
                            defer,
                        )?;
                        // Generic dispatch: a native that completes normally
                        // falls through so the post-`TailCall` releases run.
                        // Builder is left on the continue block; keep
                        // translating the rest of this LIR block.
                        self.emit_tail_call_result_branch(builder, *dst, rt, rp, park_site)?;
                        return Ok(false);
                    }
                }

                // Fallback: no self-tail-call optimization
                let (rt, rp) = self.emit_tail_call_with_args(
                    builder,
                    ft,
                    fp,
                    args,
                    vm,
                    region_id_const,
                    defer,
                )?;
                // A native that completes normally falls through to run the
                // post-`TailCall` owned-arg releases; a closure (sentinel),
                // yield, or error returns. Builder is left on the continue
                // block, so keep translating the rest of this LIR block.
                self.emit_tail_call_result_branch(builder, *dst, rt, rp, park_site)?;
                return Ok(false);
            }

            // A closure's code object is a payload in its compile unit's code
            // region, and the worker that compiles has no heap to write one
            // into (docs/impl/jit.md).
            InstrRef::MakeClosure { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeClosure".to_string()));
            }
            _ => return self.translate_instr_async(builder, instr, region_id_const),
        }
        Ok(false)
    }
}
