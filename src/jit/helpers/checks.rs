// audited: 2026-10-06
// src/jit/AGENTS.md
// docs/impl/region/unwind.md
//! The check a compiled frame runs where a signal can leave it, and the park it makes there.
//!
//! A call, or an instruction that may raise, is followed by one check. A signal
//! that reaches it parks the frame at the site, a halt included in the check
//! but given no frame, so the code after the site runs only on a clean return.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use crate::value::repr::{TAG_NIL, TAG_TRUE};

use super::super::translate::FunctionTranslator;
use super::super::JitError;

impl FunctionTranslator<'_> {
    /// Emit the check after a site a signal can leave this frame at: a call, or
    /// an instruction that may raise (`Op::may_raise`).
    ///
    /// In a function that may signal, the site is the next one the emitter
    /// recorded, and any pending signal parks the frame there: the helper parks
    /// it as a bytecode frame at the site's resume ip behind whatever the callee
    /// parked, an error included, and builds no frame for a halt
    /// (docs/impl/region/unwind.md). A silent function records no site. The only
    /// signal that reaches it is a halt or the terminal error a broken proof
    /// raises, and it returns on either (docs/signals/jit.md).
    pub(crate) fn emit_signal_check(
        &mut self,
        builder: &mut FunctionBuilder,
    ) -> Result<(), JitError> {
        let site = if self.lir.signal().may_suspend() {
            let idx = self.call_site_index;
            self.call_site_index += 1;
            Some(idx)
        } else {
            None
        };
        self.emit_signal_check_at(builder, site)
    }

    /// [`Self::emit_signal_check`] at a site already taken: `Some` parks the
    /// frame there, and `None` returns. A tail call takes its site once and
    /// checks on two branches.
    pub(crate) fn emit_signal_check_at(
        &mut self,
        builder: &mut FunctionBuilder,
        site: Option<u32>,
    ) -> Result<(), JitError> {
        let vm = self
            .vm_ptr
            .ok_or_else(|| JitError::InvalidLir("signal check without vm pointer".to_string()))?;

        let (has_sig_tag, _) = self.call_helper_vm_only(builder, self.helpers.has_signal, vm)?;
        let tag_true = builder.ins().iconst(I64, TAG_TRUE as i64);
        let is_true = builder.ins().icmp(IntCC::Equal, has_sig_tag, tag_true);

        let exit_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_true, exit_block, &[], cont_block, &[]);

        builder.switch_to_block(exit_block);
        builder.seal_block(exit_block);
        if let Some(idx) = site {
            self.emit_park_at_call_site(builder, idx)?;
        } else {
            let nil_tag = builder.ins().iconst(I64, TAG_NIL as i64);
            let zero = builder.ins().iconst(I64, 0);
            self.emit_pop_then_return(builder, nil_tag, zero)?;
        }

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Park this frame at an emitter-recorded call site and return the park's
    /// result.
    ///
    /// The builder must sit on a block that only a pending signal reaches.
    /// The block ends here: the exit pops this activation's region map after
    /// the helper has cloned it into the parked frame.
    pub(crate) fn emit_park_at_call_site(
        &mut self,
        builder: &mut FunctionBuilder,
        call_site_idx: u32,
    ) -> Result<(), JitError> {
        let vm = self.vm_ptr.ok_or_else(|| {
            JitError::InvalidLir("park at call site without vm pointer".to_string())
        })?;
        let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
            JitError::InvalidLir("park at call site without self_tag_payload".to_string())
        })?;

        // Every park must have its emitter-recorded call site: the
        // runtime helper indexes JitCode.call_sites with this same index, so
        // a missing entry means the counters diverged and the side-exit
        // would rebuild the frame from another site's stack shape.
        let stack_regs = match self.lir.call_site(call_site_idx as usize) {
            Some(cs) => cs.stack_regs,
            None => {
                return Err(JitError::InvalidLir(format!(
                    "call site {} has no emitter-recorded metadata ({} recorded)",
                    call_site_idx,
                    self.lir.call_sites().len()
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
        self.emit_pop_then_return(builder, result_tag, result_payload)
    }
}
