// audited: 2026-09-08
//! `and`/`or` lowering: the branch a short-circuit operator compiles to.
//!
//! docs/impl/region/replicate.md

use super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::lir::lower) fn lower_and(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(true));
        }
        if exprs.len() == 1 {
            return self.lower_expr(&exprs[0]);
        }

        // Allocate result slot (same pattern as lower_cond/lower_if)
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            // Store value to result slot
            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
                src: val_reg,
            });

            if i < exprs.len() - 1 {
                // Not the last expression: reload for branch test
                let cond_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cond_reg,
                    slot: result_slot,
                });

                let next_label = self.fresh_label();
                // If falsy, short-circuit to done (value already in slot)
                // If truthy, continue to next expression
                self.terminate(Terminator::Branch {
                    cond: cond_reg,
                    then_label: next_label,
                    else_label: done_label,
                });
                self.finish_block();

                self.current_block = BasicBlock::new(next_label);
            } else {
                // Last expression: jump to done (value already in slot)
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        // Done block: load result from slot
        self.current_block = BasicBlock::new(done_label);
        let result_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    pub(in crate::lir::lower) fn lower_or(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        if exprs.is_empty() {
            return self.emit_const(LirConst::Bool(false));
        }
        if exprs.len() == 1 {
            return self.lower_expr(&exprs[0]);
        }

        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let done_label = self.fresh_label();

        for (i, expr) in exprs.iter().enumerate() {
            let val_reg = self.lower_expr(expr)?;

            self.emit(LirInstr::StoreLocal {
                slot: result_slot,
                src: val_reg,
            });

            if i < exprs.len() - 1 {
                let cond_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cond_reg,
                    slot: result_slot,
                });

                let next_label = self.fresh_label();
                // If truthy, short-circuit to done
                // If falsy, continue to next expression
                self.terminate(Terminator::Branch {
                    cond: cond_reg,
                    then_label: done_label, // ← inverted from lower_and
                    else_label: next_label, // ← inverted from lower_and
                });
                self.finish_block();

                self.current_block = BasicBlock::new(next_label);
            } else {
                self.terminate(Terminator::Jump(done_label));
                self.finish_block();
            }
        }

        self.current_block = BasicBlock::new(done_label);
        let result_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }
}
