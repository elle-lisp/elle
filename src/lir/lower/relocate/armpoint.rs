// audited: 2026-11-14
//! The point each arm of a tail call's branch operand opens at its end, for a release only a sibling arm hands the call.
//!
//! docs/impl/region/operand.md
//! docs/impl/region/replicate.md

use super::super::*;
use crate::hir::region::Region;
use crate::hir::CallArg;
use rustc_hash::{FxHashMap, FxHashSet};

/// What opened a relocation point, and so how long it lives and which
/// releases it admits.
#[derive(Clone)]
pub(crate) enum PointKind {
    /// The frame-replacing tail call `call`. It dies at the next block boundary
    /// unless a merge inherits it, and admits every region it does not exempt.
    /// `postponed` holds the slot each operand arm copies a release it postpones
    /// to the call into, one per region and the slot the release reads.
    TailCall {
        call: HirId,
        postponed: FxHashMap<(Region, u16), u16>,
    },
    /// A `break`, kept exactly while the labeled block it left is open.
    Break(BlockId),
    /// The end of an arm of the branch operand of the tail call `call`. It
    /// releases only the regions the call receives on a sibling arm alone, which
    /// the frame held before the operand ran or the arm's prefix allocated. Of
    /// those, it postpones to the call each one a later operand or the callee
    /// reads. It dies with the call's own point, at a block boundary or a later
    /// tail call, and a merge inherits the two together. A branch lowered after
    /// the call in the call's block inherits neither, because `finish_block`
    /// drops the call's point.
    OperandArm {
        call: HirId,
        release: FxHashSet<Region>,
        postpone: FxHashSet<Region>,
    },
}

impl TailExitHoist {
    /// Whether a release of the merged root `root` may take a copy here.
    pub(in crate::lir::lower) fn admits(&self, root: Region) -> bool {
        match &self.kind {
            PointKind::OperandArm { release, .. } => release.contains(&root),
            PointKind::TailCall { .. } | PointKind::Break(_) => !self.exempt.contains(&root),
        }
    }

    /// The tail call this operand arm postpones a release of the merged root
    /// `root` to, if it postpones one.
    pub(in crate::lir::lower) fn postpones(&self, root: Region) -> Option<HirId> {
        match &self.kind {
            PointKind::OperandArm { call, postpone, .. } if postpone.contains(&root) => Some(*call),
            _ => None,
        }
    }

    /// Whether this is the point of the tail call `call`.
    pub(in crate::lir::lower) fn is_call(&self, id: HirId) -> bool {
        self.call() == Some(id)
    }

    /// The tail call whose own point this is.
    pub(in crate::lir::lower) fn call(&self) -> Option<HirId> {
        match self.kind {
            PointKind::TailCall { call, .. } => Some(call),
            PointKind::Break(_) | PointKind::OperandArm { .. } => None,
        }
    }

    /// The tail call this operand arm point belongs to.
    pub(in crate::lir::lower) fn arm_of(&self) -> Option<HirId> {
        match self.kind {
            PointKind::OperandArm { call, .. } => Some(call),
            PointKind::TailCall { .. } | PointKind::Break(_) => None,
        }
    }

    /// The postponed slots of this tail call's point, by region and the slot
    /// the release reads. Only a tail call's point postpones a release.
    pub(in crate::lir::lower) fn postponed_mut(&mut self) -> &mut FxHashMap<(Region, u16), u16> {
        match &mut self.kind {
            PointKind::TailCall { postponed, .. } => postponed,
            PointKind::Break(_) | PointKind::OperandArm { .. } => {
                unreachable!("only a tail call's point holds postponed slots")
            }
        }
    }

    /// The labeled block a `break`'s point left through, which bounds its life.
    pub(in crate::lir::lower) fn left_block(&self) -> Option<BlockId> {
        match self.kind {
            PointKind::Break(id) => Some(id),
            PointKind::TailCall { .. } | PointKind::OperandArm { .. } => None,
        }
    }
}

/// An arm of the tail-call operand being lowered, read before the operand
/// lowers so that the arm's end can carry what its point needs.
pub(crate) struct TailOperandArm {
    /// The operand the arm belongs to.
    pub(in crate::lir::lower) operand: HirId,
    /// The regions the operand allocates outside the arm's prefix.
    pub(in crate::lir::lower) elsewhere: FxHashSet<Region>,
}

/// Where an arm of a tail call's branch operand ended, recorded as the arm
/// closes so that the call can open a point there.
pub(crate) struct OperandArmEnd {
    /// The operand the arm belongs to, by which the call claims the end.
    operand: HirId,
    /// The regions the operand allocates outside the arm's prefix.
    elsewhere: FxHashSet<Region>,
    /// The finished block the arm ended in.
    block: usize,
    /// The end of that block's instruction list, ahead of its jump to the merge.
    at: usize,
    /// The regions the arm's value names, read as an operand's value is.
    names: FxHashSet<Region>,
}

impl<'a> Lowerer<'a> {
    /// Close a branch arm whose value is `value`: store it into the branch's
    /// result slot, then end the arm ([`Self::end_arm`]).
    pub(in crate::lir::lower) fn close_arm(
        &mut self,
        body: Option<&Hir>,
        value: Reg,
        result_slot: u16,
        merge: Label,
    ) {
        self.emit(InstrRef::StoreLocal {
            slot: result_slot,
            src: value,
        });
        self.end_arm(body, merge);
    }

    /// End a branch arm whose value is already in the branch's result slot:
    /// jump to the merge, and seal the arm's points. `body` is the arm's HIR,
    /// absent for the nil of a `cond` with no else. An arm of a tail call's
    /// branch operand records where it ended.
    pub(in crate::lir::lower) fn end_arm(&mut self, body: Option<&Hir>, merge: Label) {
        let arm = body.and_then(|b| Some((b, self.tail_operand_arms.get(&b.id)?)));
        if let Some((body, arm)) = arm {
            let mut names = FxHashSet::default();
            self.region_info.operand_value_regions(body, &mut names);
            let end = OperandArmEnd {
                operand: arm.operand,
                elsewhere: arm.elsewhere.clone(),
                block: self.built().finished_len(),
                at: self.built().open_len(),
                names,
            };
            self.operand_arm_ends.push(end);
        }
        self.terminate(Terminator::Jump(merge));
        self.seal_arm_hoists();
        self.finish_block();
    }

    /// Open a point at the end of each recorded arm of the branch operands of
    /// the tail call `call_id`, beside the call's own point, which is the last
    /// one pushed.
    ///
    /// An arm of argument `i` admits a region the call exempts through `i`
    /// alone, which the arm's own value does not name. Where a later argument
    /// or the callee reads the region anywhere in its evaluation, the arm
    /// postpones the release to the call: the arm runs before them, and the
    /// stamp guards only the slot the release reads. A region that argument
    /// `i` allocates outside the arm's prefix stays out: on a path that did not
    /// allocate it, the slot reads nil and the release does nothing. `fixed` is
    /// what the call exempts whatever its arguments name.
    pub(in crate::lir::lower) fn open_operand_arm_points(
        &mut self,
        call_id: HirId,
        func: &Hir,
        args: &[CallArg],
        fixed: &FxHashSet<Region>,
    ) {
        let call = self
            .tail_exit_hoist
            .last()
            .expect("the call's own point is open")
            .clone();
        let values: Vec<FxHashSet<Region>> = args
            .iter()
            .map(|a| {
                let mut v = FxHashSet::default();
                self.region_info.operand_value_regions(&a.expr, &mut v);
                v
            })
            .collect();
        let mut later = FxHashSet::default();
        self.region_info.evaluation_regions(func, &mut later);
        for i in (0..args.len()).rev() {
            let operand = args[i].expr.id;
            let (ends, rest): (Vec<OperandArmEnd>, Vec<OperandArmEnd>) =
                std::mem::take(&mut self.operand_arm_ends)
                    .into_iter()
                    .partition(|e| e.operand == operand);
            self.operand_arm_ends = rest;
            let (read_later, only_here): (FxHashSet<Region>, FxHashSet<Region>) = values[i]
                .iter()
                .copied()
                .filter(|r| call.exempt.contains(r) && !fixed.contains(r))
                .filter(|r| (0..args.len()).all(|j| j == i || !values[j].contains(r)))
                .partition(|r| later.contains(r));
            for end in ends {
                let ours = |set: &FxHashSet<Region>| -> FxHashSet<Region> {
                    set.iter()
                        .copied()
                        .filter(|r| !end.names.contains(r) && !end.elsewhere.contains(r))
                        .collect()
                };
                let (release, postpone) = (ours(&only_here), ours(&read_later));
                if !release.is_empty() || !postpone.is_empty() {
                    let kind = PointKind::OperandArm {
                        call: call_id,
                        release,
                        postpone,
                    };
                    let block = HoistBlock::Finished(end.block);
                    let loaded = call.loaded();
                    let point =
                        TailExitHoist::new(kind, block, end.at, loaded, FxHashSet::default());
                    self.tail_exit_hoist.push(point);
                }
            }
            self.region_info
                .evaluation_regions(&args[i].expr, &mut later);
        }
    }
}
