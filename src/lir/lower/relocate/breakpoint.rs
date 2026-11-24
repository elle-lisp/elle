// audited: 2026-11-24
//! The point a `break` opens at the end of the block it leaves, the points that cover the break, and the scope that bounds their life.
//!
//! docs/impl/region/breakpoint.md

use super::super::*;

impl TailExitHoist {
    /// The labeled block whose lowering bounds this point's life: the block a
    /// `break`'s own point left, or the block a covered `break` carried this
    /// point to.
    pub(in crate::lir::lower) fn bound_block(&self) -> Option<BlockId> {
        self.left_block().or(self.carried)
    }
}

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
    /// docs/impl/region/breakpoint.md
    pub(in crate::lir::lower) fn open_break_exit_hoist(
        &mut self,
        block_id: BlockId,
        value: &Hir,
        value_reg: Reg,
    ) {
        let loaded = self.loaded_from(|r| r == value_reg);
        let mut exempt = rustc_hash::FxHashSet::default();
        self.collect_operand_regions(value, &mut exempt);
        let block = super::HoistBlock::Finished(self.built().finished_len());
        let at = self.built().open_len();
        let point =
            super::TailExitHoist::new(PointKind::Break(block_id), block, at, loaded, exempt);
        self.tail_exit_hoist.push(point);
    }

    /// Carry the points that cover a `break` to the block `block_id` it jumps
    /// to: a tail call whose result the break carries, with its operand arms.
    ///
    /// Called where [`Self::open_break_exit_hoist`] is, before the jump. A
    /// point in the open block is sealed onto the index `finish_block` is about
    /// to give it. Another break's point covers positions this path never
    /// reaches, and a point already carried covers this break no more than it
    /// covers any position past the jump that carried it, so both are left as
    /// they are.
    ///
    /// docs/impl/region/breakpoint.md
    pub(in crate::lir::lower) fn carry_break_covering_points(&mut self, block_id: BlockId) {
        let index = self.built().finished_len();
        let label = self.built().open_label();
        for h in &mut self.tail_exit_hoist {
            if h.bound_block().is_some() {
                continue;
            }
            match h.block {
                super::HoistBlock::Current(l) if l == label => {
                    h.block = super::HoistBlock::Finished(index);
                }
                super::HoistBlock::Current(_) => continue,
                super::HoistBlock::Finished(_) => {}
            }
            h.carried = Some(block_id);
        }
    }

    /// Take out the points a `break` carried to the block `block_id`, whose
    /// exit label is about to open. The label inherits them as a branch merge
    /// inherits its arms' points, so each dies at the next block boundary.
    ///
    /// docs/impl/region/breakpoint.md
    pub(in crate::lir::lower) fn take_carried_points(
        &mut self,
        block_id: BlockId,
    ) -> Vec<super::TailExitHoist> {
        let (mut carried, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tail_exit_hoist)
            .into_iter()
            .partition(|h| h.carried == Some(block_id));
        self.tail_exit_hoist = rest;
        for h in &mut carried {
            h.carried = None;
        }
        carried
    }

    /// Drop every point whose bounding block has finished lowering. A tail
    /// call's point that no break carried has no such scope and is left alone;
    /// the block boundaries decide its life instead.
    ///
    /// docs/impl/region/breakpoint.md
    pub(in crate::lir::lower) fn retain_open_break_points(&mut self) {
        if self
            .tail_exit_hoist
            .iter()
            .all(|h| h.bound_block().is_none())
        {
            return;
        }
        let open: Vec<BlockId> = self
            .block_lower_contexts
            .iter()
            .map(|c| c.block_id)
            .collect();
        self.tail_exit_hoist
            .retain(|h| h.bound_block().is_none_or(|id| open.contains(&id)));
    }
}
