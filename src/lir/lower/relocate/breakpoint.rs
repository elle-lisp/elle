// audited: 2026-11-12
//! The point a `break` opens at the end of the block it leaves, and the scope that bounds its life.
//!
//! docs/impl/region/replicate.md

use super::super::*;

impl<'a> Lowerer<'a> {
    /// Open the relocation point a `break` leaves at the end of the block it is
    /// jumping out of.
    ///
    /// Called with the break's value already stored into the block's result slot
    /// and the jump not yet emitted, so the point names the end of an
    /// instruction list nothing else will append to. Sealed here rather than in
    /// [`Self::seal_arm_hoists`], `finish_block` being about to give this block
    /// the index `blocks.len()` names.
    ///
    /// `exempt` is the value the break CARRIES, read the same two ways a tail
    /// call's operands are.
    ///
    /// docs/impl/region/replicate.md
    pub(in crate::lir::lower) fn open_break_exit_hoist(
        &mut self,
        block_id: BlockId,
        value: &Hir,
        value_reg: Reg,
    ) {
        let (operand_locals, operand_captures) = self.loaded_from(|r| r == value_reg);
        let mut exempt = rustc_hash::FxHashSet::default();
        self.collect_operand_regions(value, &mut exempt);
        self.tail_exit_hoist.push(super::TailExitHoist {
            at: self.built().open_len(),
            block: super::HoistBlock::Finished(self.built().finished_len()),
            operand_locals,
            operand_captures,
            exempt,
            kind: PointKind::Break(block_id),
        });
    }

    /// Drop every break point whose block has finished lowering. A tail call's
    /// point has no such scope and is left alone; the block boundaries decide
    /// its life instead.
    ///
    /// docs/impl/region/replicate.md
    pub(in crate::lir::lower) fn retain_open_break_points(&mut self) {
        if self
            .tail_exit_hoist
            .iter()
            .all(|h| h.left_block().is_none())
        {
            return;
        }
        let open: Vec<BlockId> = self
            .block_lower_contexts
            .iter()
            .map(|c| c.block_id)
            .collect();
        self.tail_exit_hoist
            .retain(|h| h.left_block().is_none_or(|id| open.contains(&id)));
    }
}
