// audited: 2026-10-12
// docs/impl/jit.md
// docs/signals/inference.md
//! The inline fuel charge, which takes one unit from the fiber's budget at
//! each charge site and parks the frame when the budget is spent.
//!
//! The emitter records the sites of a function that may signal in the order
//! the translator meets them, so one counter indexes the view's charge sites
//! here and `JitCode.charge_sites` in `elle_jit_fuel_pause`. Code the compiled
//! function never runs still counts its sites, so the counters stay in step. A
//! silent function records no site: its charge spends a unit while one is left
//! and never pauses.

use super::*;
use crate::value::fiber::Fuel;

impl<'a> FunctionTranslator<'a> {
    /// Whether a jump to `target` from the block being translated runs
    /// backward in the bytecode, which is where the interpreter charges fuel.
    pub(crate) fn is_back_edge(&self, target: Label) -> bool {
        self.block_position
            .get(&target)
            .is_some_and(|&at| at <= self.current_block)
    }

    /// Whether this function records call sites and charge sites. Any signal
    /// bit, an error included, means the function may suspend, so only a
    /// silent function records none.
    fn records_sites(&self) -> bool {
        self.lir.signal().may_suspend()
    }

    /// Take the next charge site's index. A site the emitter did not record
    /// means the two walks disagree, and the compile is refused.
    fn next_charge_site(&mut self) -> Result<u32, JitError> {
        let index = self.charge_site_index;
        if index as usize >= self.lir.charge_sites().len() {
            return Err(JitError::InvalidLir(format!(
                "charge site {} has no emitter-recorded metadata ({} recorded)",
                index,
                self.lir.charge_sites().len()
            )));
        }
        self.charge_site_index += 1;
        Ok(index)
    }

    /// Count a charge site in code the compiled function never runs, such as
    /// the jump an `emit` resumes through. The interpreter reaches it, if at
    /// all, only after the frame has left compiled code.
    pub(crate) fn skip_charge_site(&mut self) -> Result<(), JitError> {
        if self.records_sites() {
            self.next_charge_site()?;
        }
        Ok(())
    }

    /// Count the sites the emitter recorded in `instrs` and `term`, which
    /// follow an instruction that left compiled code for good. Every counter
    /// stays in step with the emitter's walk, so the sites after these index
    /// their own metadata.
    pub(crate) fn skip_unreached_sites<'i>(
        &mut self,
        instrs: impl Iterator<Item = InstrRef<'i>>,
        term: &Terminator,
    ) -> Result<(), JitError> {
        let records = self.records_sites();
        for instr in instrs {
            let call_site = match instr {
                InstrRef::Call { .. }
                | InstrRef::SuspendingCall { .. }
                | InstrRef::TailCall { .. }
                | InstrRef::CallArrayMut { .. } => {
                    self.skip_charge_site()?;
                    true
                }
                InstrRef::TailCallArrayMut { .. } => {
                    self.skip_charge_site()?;
                    false
                }
                _ => instr.may_raise(),
            };
            if records && call_site {
                self.call_site_index += 1;
            }
        }
        match term {
            Terminator::Jump(label) if self.is_back_edge(*label) => self.skip_charge_site(),
            Terminator::Branch { then_label, .. } if self.is_back_edge(*then_label) => {
                self.skip_charge_site()
            }
            Terminator::Emit { resume_label, .. } => {
                self.yield_point_index += 1;
                if self.is_back_edge(*resume_label) {
                    self.skip_charge_site()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Charge one unit of fuel at the next charge site.
    ///
    /// In a function that may signal, loads the budget word through the VM
    /// pointer, pauses on zero, and otherwise stores the word less one. The
    /// pause spills the operand stack the site recorded, parks the frame at the
    /// charging instruction through `elle_jit_fuel_pause`, and returns its
    /// sentinel. In a silent function, stores the word less one unless it is
    /// zero, and nothing leaves the function. The builder is left on the block
    /// that continues past the charge.
    pub(crate) fn emit_charge(&mut self, builder: &mut FunctionBuilder) -> Result<(), JitError> {
        let vm = self
            .vm_ptr
            .ok_or_else(|| JitError::InvalidLir("fuel charge without vm pointer".to_string()))?;
        // The VM outlives every compiled activation it runs, and the word is a
        // naturally aligned u64, so the access is trusted.
        let offset = Fuel::WORD_OFFSET_IN_VM as i32;
        let flags = MemFlagsData::trusted();
        let word = builder.ins().load(I64, flags, vm, offset);

        if !self.records_sites() {
            let left = builder.ins().icmp_imm_u(IntCC::NotEqual, word, 0);
            let taken = builder.ins().uextend(I64, left);
            let spent = builder.ins().isub(word, taken);
            builder.ins().store(flags, spent, vm, offset);
            return Ok(());
        }

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
}
