// audited: 2026-11-27
//! How a call leaves compiled code: its fuel charge, the self-tail-call loop,
//! the direct self-call, the call helper, and `MakeClosure`.
//!
//! docs/impl/jit.md
//! docs/impl/jit-selfcall.md
//! docs/impl/jit-closure.md
//! docs/impl/region/merging.md

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Call/TailCall/MakeClosure instructions (chain link from translate_instr).
    pub(super) fn translate_instr_call(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &InstrRef<'_>,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<bool, JitError> {
        match instr {
            InstrRef::Call {
                dst,
                func,
                args,
                moves,
                ..
            }
            | InstrRef::SuspendingCall {
                dst,
                func,
                args,
                moves,
                ..
            } => {
                self.emit_charge(builder, Resumes::AtNode)?;
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Call without vm pointer".to_string()))?;

                let args_addr = self.spill_regs(builder, args);
                let nargs = builder.ins().iconst(I64, args.len() as i64);
                if self.self_calls.admits(*func, args, *moves) {
                    let (rt, rp) =
                        self.emit_self_call(builder, ft, fp, args_addr, nargs, region_id_const)?;
                    self.def_var_pair(builder, dst.0, rt, rp);
                    return Ok(false);
                }
                let (handoff, word) = self.handoff_word(builder, *moves);
                let (rt, rp) = self.call_helper_call(
                    builder,
                    ft,
                    fp,
                    args_addr,
                    nargs,
                    vm,
                    region_id_const,
                    handoff,
                )?;
                if let Some(word) = word {
                    self.discharge_handoff(builder, *moves, word);
                }
                self.def_var_pair(builder, dst.0, rt, rp);
            }

            InstrRef::TailCall {
                dst,
                func,
                args,
                defer_callee_release,
                deferred_release_slots,
                late_mints,
                ..
            } => {
                let (ft, fp) = self.use_var_pair(builder, func.0);
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("TailCall without vm pointer".to_string())
                })?;
                // The releases this call strands past the frame replacement. The
                // helper hands them to the activation that runs the callee, this
                // one having popped its own dues slot by then
                // (docs/impl/region/relocate.md).
                let defer = TailDeferrals::of(*defer_callee_release, *deferred_release_slots);

                // The emitter records one charge site and one call site per
                // tail call of a function that may suspend. Take both before
                // the self-call branch, so each is counted once whichever path
                // runs, and the native loop a self call becomes charges once
                // per trip.
                self.emit_charge(builder, Resumes::AtNode)?;
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

                        // The callee is a closure, so the loop makes each late
                        // mint, which the next pass releases as its parameter
                        // (docs/impl/region/handoff.md).
                        if !late_mints.is_empty() {
                            let incref = self.module.declare_func_in_func(
                                self.helpers.incref_value_region,
                                builder.func,
                            );
                            for offer in late_mints.iter() {
                                let (at, ap) = self.use_var_pair(builder, args[offer.position()].0);
                                builder.ins().call(incref, &[at, ap, vm]);
                            }
                        }
                        // Every new argument goes into the slot before any
                        // parameter is bound, so `(f b a)` swaps rather than
                        // clobbers. The loop binds them as the prologue binds a
                        // call's, and the arguments are moved. It replaces no
                        // frame and stays in this activation, so it hands
                        // `defer` nowhere: the callee IS this function, and the
                        // release the deferral would supply is its own. The
                        // region map is emptied, as the trampoline empties it
                        // for a frame-replacing callee: each entry names a slot
                        // of the last trip, whose release is dead here.
                        let args_addr = self.spill_regs(builder, args);
                        let nargs = builder.ins().iconst(I64, args.len() as i64);
                        self.bind_params(builder, args_addr, nargs, BindSite::SelfTail)?;
                        self.emit_clear_region_map(builder)?;
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
                            *late_mints,
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
                    *late_mints,
                )?;
                // A native that completes normally falls through to run the
                // post-`TailCall` owned-arg releases; a closure (sentinel),
                // yield, or error returns. Builder is left on the continue
                // block, so keep translating the rest of this LIR block.
                self.emit_tail_call_result_branch(builder, *dst, rt, rp, park_site)?;
                return Ok(false);
            }

            InstrRef::MakeClosure {
                dst,
                closure_id,
                captures,
                region,
            } => {
                let header = self
                    .lir
                    .child_of(*closure_id)
                    .and_then(|index| self.children.header(index))
                    .ok_or_else(|| {
                        JitError::InvalidLir(format!(
                            "MakeClosure of closure {} has no child-table entry",
                            closure_id.0
                        ))
                    })?;
                let vm = self.vm_ptr.ok_or_else(|| {
                    JitError::InvalidLir("MakeClosure without vm pointer".to_string())
                })?;
                let captures_addr = self.spill_regs(builder, captures);
                let count = builder.ins().iconst(I64, captures.len() as i64);
                let region_val = self.emit_resolve_alloc_region(builder, *region)?;
                let child_tag = builder.ins().iconst(I64, header.tag as i64);
                let child_payload = builder.ins().iconst(I64, header.payload as i64);
                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.make_closure, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[
                        captures_addr,
                        count,
                        child_tag,
                        child_payload,
                        region_val,
                        vm,
                    ],
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
