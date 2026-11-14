// audited: 2026-11-13
//! The release an operand arm postpones to its tail call: the arm's copy of the release's slot, and the `DecrefPostponed` ahead of the call.
//!
//! docs/impl/region/operand.md

use super::super::*;
use crate::hir::region::Region;

impl<'a> Lowerer<'a> {
    /// Postpone the release of the merged root `root`, which reads and stamps
    /// slot `route`, from each operand arm that postpones it to the tail call
    /// that arm belongs to.
    ///
    /// Each such arm ends by copying `route` into the call's postponed slot for
    /// `root`, and one `DecrefPostponed` naming both slots goes just ahead of
    /// the call's `TailCall`. The copy takes no count, and no release table
    /// names the postponed slot, so `route` stays the receipt an abandoned
    /// frame's walk reads.
    ///
    /// A call whose point is gone, or whose operands load `route` itself,
    /// postpones nothing, which over-keeps.
    pub(super) fn postpone_arm_releases(&mut self, root: Region, route: u16) {
        let mut calls: Vec<HirId> = Vec::new();
        for h in &self.tail_exit_hoist {
            if let (Some(call), super::HoistBlock::Finished(_)) = (h.postpones(root), h.block) {
                if !calls.contains(&call) {
                    calls.push(call);
                }
            }
        }
        for call in calls {
            let Some(c) = self.tail_exit_hoist.iter().position(|h| h.is_call(call)) else {
                continue;
            };
            if self.tail_exit_hoist[c].operand_locals.contains(&route) {
                continue;
            }
            let slot = self.postponed_slot(c, root);
            for i in 0..self.tail_exit_hoist.len() {
                let h = &self.tail_exit_hoist[i];
                let super::HoistBlock::Finished(block) = h.block else {
                    continue;
                };
                if h.postpones(root) != Some(call) {
                    continue;
                }
                let at = h.at;
                debug_assert_eq!(
                    at,
                    self.built().finished_block_len(block),
                    "an operand arm's point must still name the end of its block"
                );
                let start = self.built().open_len();
                let copy = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst: copy,
                    slot: route,
                });
                self.emit(InstrRef::StoreLocal { slot, src: copy });
                self.tail_exit_hoist[i].at += 2;
                self.b().move_run_to(start, block, at);
            }
            let at = self.tail_exit_hoist[c].at;
            let start = self.built().open_len();
            self.emit(InstrRef::DecrefPostponed { slot, route });
            self.tail_exit_hoist[c].at += 1;
            match self.tail_exit_hoist[c].block {
                super::HoistBlock::Current(_) => self.b().move_run(start, at),
                super::HoistBlock::Finished(block) => {
                    debug_assert!(
                        matches!(
                            self.built().finished_instr(block, at),
                            Some(InstrRef::TailCall { .. } | InstrRef::TailCallArrayMut { .. })
                        ),
                        "a tail call's point must still name its TailCall"
                    );
                    self.b().move_run_to(start, block, at);
                }
            }
        }
    }

    /// The postponed slot of the tail call at point `c` for `root`, minted on
    /// first use. Every arm of the call that postpones `root` writes this one
    /// slot.
    fn postponed_slot(&mut self, c: usize, root: Region) -> u16 {
        if let super::PointKind::TailCall { postponed, .. } = &self.tail_exit_hoist[c].kind {
            if let Some(&slot) = postponed.get(&root) {
                return slot;
            }
        }
        let slot = self.fresh_local();
        if let super::PointKind::TailCall { postponed, .. } = &mut self.tail_exit_hoist[c].kind {
            postponed.insert(root, slot);
        }
        slot
    }
}
