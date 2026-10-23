// audited: 2026-10-23
// docs/impl/jit-fuel.md
// docs/signals/inference.md
// benches/AGENTS.md
//! The inline fuel charge: the kill check, one unit off the fiber's budget,
//! and the park when the budget is spent.
//!
//! The emitter records the sites of a function that may signal in the order
//! the translator meets them, so one counter indexes the view's charge sites
//! here and `JitCode.charge_sites` in `elle_jit_fuel_pause`. Code the compiled
//! function never runs still counts its sites, so the counters stay in step. A
//! silent function records no site: its charge spends a unit while one is left
//! and never pauses. Every charge opens with the kill check, which leaves the
//! function on a budget below the kill word. A build with the `ablation`
//! feature lets the fuel bench choose the silent charge's shape.

use super::*;
use crate::value::fiber::Fuel;

#[cfg(feature = "ablation")]
pub(crate) mod ablation;
mod sites;

impl<'a> FunctionTranslator<'a> {
    /// Charge one unit of fuel at the next charge site.
    ///
    /// Every charge reads the kill word first, and a budget below it leaves
    /// the function through `emit_kill_check`. In a function that may signal,
    /// the charge then pauses on zero, and otherwise stores the word less one.
    /// The pause spills the operand stack the site recorded, parks the frame at
    /// the charging instruction through `elle_jit_fuel_pause`, and returns its
    /// sentinel. In a silent function, stores the word less one unless it is
    /// zero, and nothing but the kill leaves the function. The builder is left
    /// on the block that continues past the charge.
    pub(crate) fn emit_charge(&mut self, builder: &mut FunctionBuilder) -> Result<(), JitError> {
        let vm = self
            .vm_ptr
            .ok_or_else(|| JitError::InvalidLir("fuel charge without vm pointer".to_string()))?;
        if !self.records_sites() {
            return self.emit_silent_charge(builder, vm);
        }
        let (offset, flags) = Self::budget_word();
        let word = builder.ins().load(I64, flags, vm, offset);
        self.emit_kill_check(builder, vm, word)?;

        let index = self.next_charge_site()?;
        let (self_tag, self_payload) = self.self_tag_payload.ok_or_else(|| {
            JitError::InvalidLir("fuel charge without self_tag_payload".to_string())
        })?;

        let pause_block = builder.create_block();
        let charge_block = builder.create_block();
        builder.set_cold_block(pause_block);
        builder
            .ins()
            .brif(word, charge_block, &[], pause_block, &[]);

        builder.switch_to_block(pause_block);
        builder.seal_block(pause_block);
        let site = self
            .lir
            .charge_site(index as usize)
            .expect("next_charge_site checked the index");
        let spilled_ptr = self.spill_locals_and_operands(builder, site.stack_regs)?;
        let index_val = builder.ins().iconst(I64, index as i64);
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.fuel_pause, builder.func);
        let call = builder.ins().call(
            func_ref,
            &[spilled_ptr, index_val, vm, self_tag, self_payload],
        );
        let result_tag = builder.inst_results(call)[0];
        let result_payload = builder.inst_results(call)[1];
        self.emit_pop_then_return(builder, result_tag, result_payload)?;

        builder.switch_to_block(charge_block);
        builder.seal_block(charge_block);
        let less_one = builder.ins().iadd_imm_s(word, -1);
        builder.ins().store(flags, less_one, vm, offset);
        Ok(())
    }

    /// Where compiled code finds the budget word: its offset from the VM
    /// pointer, and the flags of the access. The VM outlives every compiled
    /// activation it runs, and the word is a naturally aligned u64, so the
    /// access is trusted.
    fn budget_word() -> (i32, MemFlagsData) {
        (Fuel::WORD_OFFSET_IN_VM as i32, MemFlagsData::trusted())
    }

    /// The kill check that opens every charge: load the pointer to the kill
    /// word through the VM, load the word through it, and branch to a cold
    /// block when the budget `word` is below it. The cold block raises the
    /// terminal `:killed` error through `elle_jit_kill`, releases what the
    /// activation owes as a `Return` does, and returns `nil`, spilling nothing
    /// and parking no frame (docs/impl/jit-fuel.md). The builder is left on the
    /// block that continues past the check.
    ///
    /// The word is an atomic another thread stores, read here as a plain
    /// load: the budget store beside it keeps the load in the loop, and a
    /// check that reads a stale word lands the kill one charge late.
    fn emit_kill_check(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: cranelift_codegen::ir::Value,
        word: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        let flags = MemFlagsData::trusted();
        let kill_ptr = builder
            .ins()
            .load(I64, flags, vm, Fuel::KILL_PTR_OFFSET_IN_VM as i32);
        let limit = builder.ins().load(I64, flags, kill_ptr, 0);
        let below = builder.ins().icmp(IntCC::UnsignedLessThan, word, limit);

        let kill_block = builder.create_block();
        let live_block = builder.create_block();
        builder.set_cold_block(kill_block);
        builder.ins().brif(below, kill_block, &[], live_block, &[]);

        builder.switch_to_block(kill_block);
        builder.seal_block(kill_block);
        let func_ref = self
            .module
            .declare_func_in_func(self.helpers.kill, builder.func);
        let call = builder.ins().call(func_ref, &[vm]);
        let result_tag = builder.inst_results(call)[0];
        let result_payload = builder.inst_results(call)[1];
        self.emit_release_activation_dues(builder)?;
        self.emit_pop_then_return(builder, result_tag, result_payload)?;

        builder.switch_to_block(live_block);
        builder.seal_block(live_block);
        Ok(())
    }

    /// The silent charge every build ships: the kill check, then a branch on
    /// zero around the decrement, so the word stops at zero and nothing but
    /// the kill leaves the function.
    pub(super) fn emit_checked_silent_charge(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        let (offset, flags) = Self::budget_word();
        let word = builder.ins().load(I64, flags, vm, offset);
        self.emit_kill_check(builder, vm, word)?;
        self.emit_spend_unless_zero(builder, vm, word);
        Ok(())
    }

    /// The silent charge's spend: a branch on zero around the decrement of
    /// the budget `word`. The builder is left on the block past the branch.
    pub(super) fn emit_spend_unless_zero(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: cranelift_codegen::ir::Value,
        word: cranelift_codegen::ir::Value,
    ) {
        let (offset, flags) = Self::budget_word();
        let spend = builder.create_block();
        let done = builder.create_block();
        builder.ins().brif(word, spend, &[], done, &[]);

        builder.switch_to_block(spend);
        builder.seal_block(spend);
        let less_one = builder.ins().iadd_imm_s(word, -1);
        builder.ins().store(flags, less_one, vm, offset);
        builder.ins().jump(done, &[]);

        builder.switch_to_block(done);
        builder.seal_block(done);
    }

    /// The charge at a silent site, which is the checked one in every build
    /// but the bench's.
    #[cfg(not(feature = "ablation"))]
    fn emit_silent_charge(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: cranelift_codegen::ir::Value,
    ) -> Result<(), JitError> {
        self.emit_checked_silent_charge(builder, vm)
    }
}
