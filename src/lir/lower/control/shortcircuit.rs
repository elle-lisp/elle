// audited: 2026-11-13
//! `and`/`or` lowering: the branch a short-circuit operator compiles to.
//!
//! docs/impl/region/replicate.md
//! docs/impl/region/operand.md

use super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_and(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(ConstRef::Bool(true));
        }
        self.lower_short_circuit(exprs, false)
    }

    pub(in crate::lir::lower) fn lower_or(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(ConstRef::Bool(false));
        }
        self.lower_short_circuit(exprs, true)
    }

    /// Lower the operands of an `and` (`stops_on` false) or an `or` (`stops_on`
    /// true). Each operand stores its value into the result slot, and each but
    /// the last branches on it: to the done block where its truth is `stops_on`,
    /// to the next operand otherwise.
    ///
    /// Every operand's block reaches the done block, so each is an ARM of the
    /// branch this writes and the done block is their merge
    /// (docs/impl/region/replicate.md). An operand that is an arm of a tail
    /// call's operand reaches it through a block of its own, the stopping edge,
    /// whose end is where that arm ends (docs/impl/region/operand.md).
    fn lower_short_circuit(&mut self, exprs: &[Hir], stops_on: bool) -> Result<Reg, String> {
        if let [only] = exprs {
            return self.lower_expr(only);
        }

        // Allocate result slot (same pattern as lower_cond/lower_if)
        let result_slot = self.fresh_local();
        let done_label = self.fresh_label();

        // Read while this branch's own entry block is still open, exactly as
        // `lower_if` and `lower_cond` read it.
        let branch_hoists = self.begin_branch_arms();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            // Store value to result slot
            self.emit(InstrRef::StoreLocal {
                slot: result_slot,
                src: val_reg,
            });

            if i == exprs.len() - 1 {
                // Last expression: jump to done (value already in slot)
                self.end_arm(Some(expr), done_label);
                continue;
            }
            // Not the last expression: reload for branch test
            let cond_reg = self.fresh_reg();
            self.emit(InstrRef::LoadLocal {
                dst: cond_reg,
                slot: result_slot,
            });

            let next_label = self.fresh_label();
            let stop_label = if self.tail_operand_arms.contains(&expr.id) {
                self.fresh_label()
            } else {
                done_label
            };
            let (then_label, else_label) = if stops_on {
                (stop_label, next_label)
            } else {
                (next_label, stop_label)
            };
            self.terminate(Terminator::Branch {
                cond: cond_reg,
                then_label,
                else_label,
            });
            self.seal_arm_hoists();
            self.finish_block();

            if stop_label != done_label {
                self.open_block(stop_label);
                self.end_arm(Some(expr), done_label);
            }
            self.open_block(next_label);
        }

        // Done block: load result from slot
        self.open_block(done_label);
        self.open_branch_merge(branch_hoists);
        let result_reg = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }
}
