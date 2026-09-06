// audited: 2026-09-06
// src/jit/AGENTS.md
// docs/impl/region/mechanism.md
//! The two checks a compiled call site runs on the way back.
//!
//! A callee that raised abandons this activation; a callee that suspended parks
//! it. Each check is a branch and an exit block spliced in after the call, so
//! the caller continues only on the normal return.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use crate::value::repr::{TAG_NIL, TAG_TRUE};

use super::super::translate::FunctionTranslator;
use super::super::JitError;

impl FunctionTranslator<'_> {
    /// Emit exception check after a call instruction.
    ///
    /// The callee raised, so this activation is abandoned where it stands: it
    /// runs the releases it still owed, pops its region-remap frame, and returns
    /// (TAG_NIL, 0) for the caller's own check to find
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes").
    pub(crate) fn emit_exception_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_exception_check without vm pointer".to_string())
        })?;

        // Call has_exception(vm) -> (tag, payload)
        let (has_exc_tag, _) = self.call_helper_vm_only(builder, self.helpers.has_exception, vm)?;

        // TAG_TRUE = 3, TAG_FALSE = 4, TAG_NIL = 2 — check if tag == TAG_TRUE
        let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
        let is_true = builder.ins().icmp(IntCC::Equal, has_exc_tag, tag_true);

        let exc_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(is_true, exc_block, &[], cont_block, &[]);

        builder.switch_to_block(exc_block);
        builder.seal_block(exc_block);
        let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
        let zero = builder.ins().iconst(I64, 0);
        self.emit_abandoned_error_return(builder, nil_tag, zero)?;

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Emit yield check after a call instruction for yielding functions.
    ///
    /// The callee suspended, so this activation suspends with it: the helper
    /// parks the frame — reading this activation's region map, still on top —
    /// and the exit then pops that map like every other one
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). Nothing is released here: the frame resumes, and the
    /// releases it still owes are the resumed body's to run.
    pub(crate) fn emit_yield_check_after_call(
        &mut self,
        builder: &mut FunctionBuilder,
        call_site_idx: u32,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without vm pointer".to_string())
        })?;
        let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
            JitError::InvalidLir("emit_yield_check without self_tag_payload".to_string())
        })?;

        // Check if any signal is pending
        let (has_sig_tag, _) = self.call_helper_vm_only(builder, self.helpers.has_signal, vm)?;
        let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
        let is_true = builder.ins().icmp(IntCC::Equal, has_sig_tag, tag_true);

        let yield_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_true, yield_block, &[], cont_block, &[]);

        builder.switch_to_block(yield_block);
        builder.seal_block(yield_block);

        // Every yield check must have its emitter-recorded call site: the
        // runtime helper indexes JitCode.call_sites with this same index, so
        // a missing entry means the counters diverged and the side-exit
        // would rebuild the frame from another site's stack shape.
        let stack_regs = match self.lir.call_sites.get(call_site_idx as usize) {
            Some(cs) => cs.stack_regs.as_slice(),
            None => {
                return Err(JitError::InvalidLir(format!(
                    "call site {} has no emitter-recorded metadata ({} recorded)",
                    call_site_idx,
                    self.lir.call_sites.len()
                )))
            }
        };

        let spilled_ptr = self.spill_locals_and_operands(builder, stack_regs)?;
        let call_site_idx_val = builder.ins().iconst(I64, call_site_idx as i64);

        // Call elle_jit_yield_through_call(spilled, call_site_index, vm, closure_tag, closure_payload)
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.jit_yield_through_call, builder.func);
        let call = builder.ins().call(
            func_ref,
            &[spilled_ptr, call_site_idx_val, vm, self_tag, self_payload],
        );
        let result_tag = builder.inst_results(call)[0];
        let result_payload = builder.inst_results(call)[1];
        self.emit_pop_then_return(builder, result_tag, result_payload)?;

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }
}
