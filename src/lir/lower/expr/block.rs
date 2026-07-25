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
        self.finish_block();

        // Else block: store result to slot, jump to merge
        self.current_block = BasicBlock::new(else_label);
        let else_reg = self.lower_expr(else_branch)?;
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: else_reg,
        });
        self.terminate(Terminator::Jump(merge_label));
        self.finish_block();

        // Merge block: load result from slot
        self.current_block = BasicBlock::new(merge_label);
        self.emit(LirInstr::LoadLocal {
            dst: result_reg,
            slot: result_slot,
        });

        Ok(result_reg)
    }

    pub(super) fn lower_block(
        &mut self,
        block_id: &BlockId,
        body: &[Hir],
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let result_reg = self.fresh_reg();
        let block_result_slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        let exit_label = self.fresh_label();
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };

        // Record active_region_ids depth before the block so breaks
        // can emit FreeRegion for regions entered since.
        let region_stack_depth = self.active_region_ids.len();

        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        self.block_lower_contexts.push(BlockLowerContext {
            block_id: *block_id,
            result_reg,
            result_slot: block_result_slot,
            exit_label,
            region_depth_at_entry: region_stack_depth as u32,
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

        // Region-demise DecrefRegion is emitted by `lower_expr` at each
        // region's `decref_point` HirId — for a value a `break` carried out,
        // that point is this Block node or later, so its release lands after
        // the exit label below and fires on both paths
        // (docs/impl/region/mechanism.md § "`break` transfers its value").
        // This function emits none; it only keeps the active_region_ids
        // bookkeeping a per-path release of the break-skipped regions would
        // walk.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }

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
