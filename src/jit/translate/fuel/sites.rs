// audited: 2026-10-22
//! The charge-site counter: which sites a function records, and how the
//! translator keeps its count in step with the emitter's walk.
//!
//! docs/impl/jit-fuel.md

use super::*;

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
    pub(super) fn records_sites(&self) -> bool {
        self.lir.signal().may_suspend()
    }

    /// Take the next charge site's index. A site the emitter did not record
    /// means the two walks disagree, and the compile is refused.
    pub(super) fn next_charge_site(&mut self) -> Result<u32, JitError> {
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
}
