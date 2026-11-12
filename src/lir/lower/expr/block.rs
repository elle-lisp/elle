// audited: 2026-11-12
//! `If` and `Block` lowering — the branch/labeled-block result-slot pattern.
//!
//! docs/impl/region/replicate.md
//! docs/impl/region/anchors.md
//!
//! Grouped because both allocate a result slot, drive control through fresh
//! labels, store each arm's result into that slot, and reload it at the merge or
//! exit. That shape is distinct from the flat `lower_begin` sequence.

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
        let result_slot = self.fresh_local();

        let then_label = self.fresh_label();
        let else_label = self.fresh_label();
        let merge_label = self.fresh_label();

        // Each arm seals whatever relocation point it ends on, so a release
        // emitted past the merge can be replicated back into the arms that leave
        // through a frame-replacing tail call (docs/impl/region/replicate.md). A
        // merge also inherits what covered the branch's entry, the points already
        // covering this position. `finish_block` clears those, so they are read
        // BEFORE the condition block closes. `cond`
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
        self.open_block(then_label);
        let then_reg = self.lower_expr(then_branch)?;
        self.close_arm(Some(then_branch), then_reg, result_slot, merge_label);

        // Else block: store result to slot, jump to merge
        self.open_block(else_label);
        let else_reg = self.lower_expr(else_branch)?;
        self.close_arm(Some(else_branch), else_reg, result_slot, merge_label);

        // Merge block: load result from slot
        self.open_block(merge_label);
        self.open_branch_merge(branch_hoists);
        self.emit(InstrRef::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    /// A `block` needs no scope region of its own, because nothing is stamped
    /// with it. The solver's `decref_point` anchors every region the block's
    /// control flow affects on the `Block` node. `lower_expr` releases those
    /// after this function returns, hence after the exit label.
    pub(super) fn lower_block(&mut self, block_id: &BlockId, body: &[Hir]) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let block_result_slot = self.fresh_local();
        let exit_label = self.fresh_label();

        self.block_lower_contexts.push(BlockLowerContext {
            block_id: *block_id,
            result_reg,
            result_slot: block_result_slot,
            exit_label,
        });

        // Lower body
        if body.is_empty() {
            let nil_reg = self.emit_const(ConstRef::Nil)?;
            self.emit(InstrRef::StoreLocal {
                slot: block_result_slot,
                src: nil_reg,
            });
        } else {
            let mut last_reg = self.lower_expr(&body[0])?;
            for expr in body.iter().skip(1) {
                self.discard(last_reg);
                last_reg = self.lower_expr(expr)?;
            }
            self.emit(InstrRef::StoreLocal {
                slot: block_result_slot,
                src: last_reg,
            });
        }

        self.block_lower_contexts.pop();

        // `lower_expr` emits each region-demise DecrefRegion at its region's
        // `decref_point` HirId. Every region a `break` affects has this Block
        // node or later as that point: the value it carried out, and every
        // release its jump passed over (docs/impl/region/anchors.md). Both
        // therefore land after the exit label below and fire on both paths, so
        // this function emits no region instruction of its own.

        // Normal exit: jump to the exit label
        self.terminate(Terminator::Jump(exit_label));
        self.start_new_block(exit_label);
        self.emit(InstrRef::LoadLocal {
            dst: result_reg,
            slot: block_result_slot,
        });

        Ok(result_reg)
    }
}
