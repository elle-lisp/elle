//! `If` and `Block` lowering — the branch/labeled-block result-slot pattern.
//!
//! Grouped because both allocate a result slot, drive control through fresh
//! labels, store each arm's result into that slot, and reload it at the merge/
//! exit — the same shape, distinct from the flat `lower_begin` sequence.

use super::*;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_if(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
    ) -> Result<Reg, String> {
        let cond_reg = self.lower_expr(cond)?;

        // Allocate result slot (same pattern as lower_cond)
        let result_reg = self.fresh_reg();
        let result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;

        let then_label = self.fresh_label();
        let else_label = self.fresh_label();
        let merge_label = self.fresh_label();

        // Each arm seals whatever relocation point it ends on, so a release
        // emitted past the merge can be replicated back into the arms that leave
        // through a frame-replacing tail call (docs/impl/region/mechanism.md
        // § "The relocation point outlives the block"). Read BEFORE the condition
        // block closes, because the merge's other source is the set of points
        // already covering this position, and `finish_block` clears them
        // (§ "A merge inherits what covered the branch's ENTRY as well"). `cond`
        // is lowered above, so nothing of this branch's own is lost by reading
        // here; `lower_cond` and `lower_match` reach the same moment with their
        // entry block still open.
        let branch_hoists = self.begin_branch_arms();

        // Terminate current block with branch
        self.terminate(Terminator::Branch {
            cond: cond_reg,
            then_label,
            else_label,
        });
        self.finish_block();

        // Then block: store result to slot, jump to merge
        self.current_block = BasicBlock::new(then_label);
        let then_reg = self.lower_expr(then_branch)?;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: then_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.seal_arm_hoists();
        self.finish_block();

        // Else block: store result to slot, jump to merge
        self.current_block = BasicBlock::new(else_label);
        let else_reg = self.lower_expr(else_branch)?;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: else_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.seal_arm_hoists();
        self.finish_block();

        // Merge block: load result from slot
        self.current_block = BasicBlock::new(merge_label);
        self.open_branch_merge(branch_hoists);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    /// A `block` needs no scope region of its own: nothing is stamped with it,
    /// and every region the block's control flow affects is anchored on the
    /// `Block` node by the solver's `decref_point`, released by `lower_expr`
    /// after this function returns (hence after the exit label).
    pub(super) fn lower_block(&mut self, block_id: &BlockId, body: &[Hir]) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let block_result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let exit_label = self.fresh_label();

        self.block_lower_contexts.push(BlockLowerContext {
            block_id: *block_id,
            result_reg,
            result_slot: block_result_slot,
            exit_label,
        });

        // Lower body
        if body.is_empty() {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            self.emit(LirInstr::StoreLocal {
                slot: block_result_slot,
                src: nil_reg,
            });
        } else {
            let mut last_reg = self.lower_expr(&body[0])?;
            for expr in body.iter().skip(1) {
                self.discard(last_reg);
                last_reg = self.lower_expr(expr)?;
            }
            self.emit(LirInstr::StoreLocal {
                slot: block_result_slot,
                src: last_reg,
            });
        }

        self.block_lower_contexts.pop();

        // Region-demise DecrefRegion is emitted by `lower_expr` at each region's
        // `decref_point` HirId — and every region a `break` affects has this
        // Block node or later as that point: the value it carried out
        // (docs/impl/region/mechanism.md § "`break` transfers its value") and
        // every release its jump passed over (§ "A release the break jumps over
        // is not a release"). Both therefore land after the exit label below and
        // fire on both paths, so this function emits no region instruction of
        // its own.

        // Normal exit: jump to the exit label
        self.terminate(Terminator::Jump(exit_label));
        self.start_new_block(exit_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: block_result_slot,
        });

        Ok(result_reg)
    }
}
