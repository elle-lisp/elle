//! Terminator translation and the generic tail-call result branch.
//!
//! `translate_terminator` lowers each LIR `Terminator`; the two tail-call
//! helpers implement the interpreter's native-vs-closure tail dispatch (the
//! Inc4 native-tail trick) that a generic `TailCall` in `instr::calls` relies on.

use super::*;

impl<'a> FunctionTranslator<'a> {
    /// Branch on a generic (helper-dispatched) tail call's runtime result so
    /// the JIT mirrors the interpreter's `tail_call_inner` (src/vm/call.rs):
    ///
    /// - `TAIL_CALL_SENTINEL` (callee was a closure → the trampoline runs it),
    ///   `YIELD_SENTINEL` (a yielding native side-exited), or a pending error:
    ///   pop the region map and return the value, exactly as before. The
    ///   post-`TailCall` owned-arg releases do NOT run — for a closure that is
    ///   the ownership MOVE (the owned-param callee releases the moved args),
    ///   and on error/yield the frame unwinds/suspends.
    /// - any other value: the callee was a NATIVE (or a parameter/collection)
    ///   that completed normally. Bind `dst` and fall through so the caller
    ///   keeps translating the post-`TailCall` block — the compiler's own
    ///   per-arg `DecrefValueRegion`/`DecrefRegion`s that release each moved
    ///   native arg. This is the Inc4 native-tail trick the interpreter
    ///   performs by NOT replacing the frame for a normally-completing native;
    ///   without it the moved arg leaks (region-native-tail-move.lisp;
    ///   docs/impl/region/rules.md Rule 8).
    ///
    /// On return the builder is positioned on the continue (fall-through)
    /// block, with `dst` defined.
    // `pub(super)` (was private in the translate root): the sibling
    // `instr::calls` submodule calls this; widen to the minimal
    // `translate`-scoped visibility so it stays reachable after the move.
    pub(super) fn emit_tail_call_result_branch(
        &mut self,
        builder: &mut FunctionBuilder,
        dst: Reg,
        rt: cranelift_codegen::ir::Value,
        rp: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        use crate::jit::value::{TAIL_CALL_SENTINEL_JV, YIELD_SENTINEL_JV};

        // result == TAIL_CALL_SENTINEL || result == YIELD_SENTINEL? The sentinel
        // tags are 0xDEAD_…_DEAD — unrepresentable as a real Value tag (> the
        // max tag), so the tag alone identifies a sentinel (the payload mirrors
        // the tag, no need to compare it).
        let tail_tag = builder.ins().iconst(I64, TAIL_CALL_SENTINEL_JV.tag as i64);
        let yield_tag = builder.ins().iconst(I64, YIELD_SENTINEL_JV.tag as i64);
        let is_tail = builder.ins().icmp(IntCC::Equal, rt, tail_tag);
        let is_yield = builder.ins().icmp(IntCC::Equal, rt, yield_tag);
        let is_sentinel = builder.ins().bor(is_tail, is_yield);

        let return_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_sentinel, return_block, &[], cont_block, &[]);

        // Closure-trampoline / yield side-exit: return the value unchanged.
        builder.switch_to_block(return_block);
        builder.seal_block(return_block);
        self.emit_pop_then_return(builder, rt, rp)?;

        // Native (or param/collection) completed normally: bind dst, propagate
        // any pending error, then fall through into the post-`TailCall` block.
        // `emit_exception_check_after_call` returns nil if the native set an
        // error signal and leaves the builder on its own continue block.
        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);
        self.def_var_pair(builder, dst.0, rt, rp);
        self.emit_exception_check_after_call(builder)?;
        Ok(())
    }

    /// Translate a terminator
    pub(crate) fn translate_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match term {
            Terminator::Return(reg) => {
                let (tag, payload) = self.use_var_pair(builder, reg.0);
                // Free this activation's owner node at normal completion — the
                // JIT twin of the interpreter trampoline's clean-break release
                // (docs/impl/region/owner.md § "Owner nodes"). Emitted before
                // the region-map pop, mirroring the interpreter's ordering, and
                // only for a function whose LIR can mint a node.
                if self.uses_activation_owner_node {
                    let vm = self.vm_ptr.ok_or_else(|| {
                        JitError::InvalidLir("owner-node release without vm pointer".to_string())
                    })?;
                    let func_ref = self.module.declare_func_in_func(
                        self.helpers.release_activation_owner_node,
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[vm]);
                }
                self.emit_pop_then_return(builder, tag, payload)?;
            }

            Terminator::Jump(label) => {
                let target = block_map.get(label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown jump target: {:?}", label))
                })?;
                builder.ins().jump(*target, &[]);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let (cond_tag, _) = self.use_var_pair(builder, cond.0);
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Truthiness: tag != TAG_NIL (2) AND tag != TAG_FALSE (4)
                // Equivalently: is_truthy if tag != NIL and tag != FALSE.
                // Simple check: tag == TAG_FALSE || tag == TAG_NIL → falsy
                let tag_nil = builder.ins().iconst(I64, TAG_NIL as i64);
                let tag_false = builder.ins().iconst(I64, TAG_FALSE as i64);
                let is_nil = builder.ins().icmp(IntCC::Equal, cond_tag, tag_nil);
                let is_false = builder.ins().icmp(IntCC::Equal, cond_tag, tag_false);
                let is_falsy = builder.ins().bor(is_nil, is_false);
                // brif on is_falsy goes to else, otherwise then
                builder
                    .ins()
                    .brif(is_falsy, *else_block, &[], *then_block, &[]);
            }

            Terminator::Emit {
                signal,
                value,
                resume_label: _,
            } => {
                let (yt, yp) = self.use_var_pair(builder, value.0);
                let vm = self
                    .vm_ptr
                    .ok_or_else(|| JitError::InvalidLir("Emit without vm pointer".to_string()))?;
                let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
                    JitError::InvalidLir("Emit without self_tag_payload".to_string())
                })?;

                let yield_index = self.yield_point_index;
                self.yield_point_index += 1;

                // Every Emit terminator must have its emitter-recorded yield
                // point: elle_jit_yield indexes JitCode.yield_points with this
                // same index, so a missing entry means the counters diverged
                // and the side-exit would resume at another point's ip.
                let stack_regs = match self.lir.yield_points.get(yield_index as usize) {
                    Some(yp) => yp.stack_regs.as_slice(),
                    None => {
                        return Err(JitError::InvalidLir(format!(
                            "yield point {} has no emitter-recorded metadata ({} recorded)",
                            yield_index,
                            self.lir.yield_points.len()
                        )))
                    }
                };

                let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;
                let yield_idx_val = builder.ins().iconst(I64, yield_index as i64);

                let sig_val = builder.ins().iconst(I64, signal.raw() as i64);

                let func_ref = self
                    .module
                    .declare_func_in_func(self.helpers.jit_yield, builder.func);
                let call = builder.ins().call(
                    func_ref,
                    &[
                        yt,
                        yp,
                        spilled_ptr,
                        yield_idx_val,
                        vm,
                        self_tag,
                        self_payload,
                        sig_val,
                    ],
                );
                let rt = builder.inst_results(call)[0];
                let rp = builder.inst_results(call)[1];
                // An ERROR emit parks no frame — nothing resumes to run the rest
                // of these instructions — so this activation is abandoned and
                // runs the releases it still owed. The signal is a compile-time
                // constant, so a suspending emit skips the walk's spill entirely
                // rather than paying for a call that would decline it.
                //
                // The walk goes AFTER `elle_jit_yield`, which installs the
                // payload and records its mint, so it reads both. The pop goes
                // after them both: the suspend helper has already cloned the live
                // activation map into the resume frame.
                if signal.intersects(crate::value::fiber::SIG_ERROR) {
                    self.emit_abandoned_error_return(builder, rt, rp)?;
                } else {
                    self.emit_pop_then_return(builder, rt, rp)?;
                }
            }

            Terminator::Unreachable => {
                // User trap code 1 — `unwrap_user(0)` panics (Cranelift user
                // trap codes are `NonZeroU8`). Reachable now that a generic
                // tail call can fall through to its block's terminator instead
                // of self-terminating (the native-tail continue path); a
                // genuinely-unreachable block must still compile to a valid
                // trap.
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            }
        }
        Ok(())
    }

    /// Helper: emit a tail call with args spilled to stack.
    // `pub(super)` (was private in the translate root): the sibling
    // `instr::calls` submodule calls this; widen to the minimal
    // `translate`-scoped visibility so it stays reachable after the move.
    pub(super) fn emit_tail_call_with_args(
        &mut self,
        builder: &mut FunctionBuilder,
        ft: cranelift_codegen::ir::Value,
        fp: cranelift_codegen::ir::Value,
        args: &[Reg],
        vm: cranelift_codegen::ir::Value,
        region_id_const: cranelift_codegen::ir::Value,
    ) -> Result<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value), JitError> {
        if args.is_empty() {
            let null_ptr = builder.ins().iconst(I64, 0);
            let nargs = builder.ins().iconst(I64, 0);
            self.call_helper_tail_call(builder, ft, fp, null_ptr, nargs, vm, region_id_const)
        } else {
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
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
            self.call_helper_tail_call(builder, ft, fp, args_addr, nargs, vm, region_id_const)
        }
    }
}
