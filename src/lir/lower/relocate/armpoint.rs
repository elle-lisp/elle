// audited: 2026-11-12
//! The point each arm of a tail call's branch operand opens at its end, for a release only a sibling arm hands the call.
//!
//! docs/impl/region/operand.md
//! docs/impl/region/replicate.md

use super::super::*;
use crate::hir::region::Region;
use crate::hir::CallArg;
use rustc_hash::FxHashSet;

/// What opened a relocation point, and so how long it lives and which
/// releases it admits.
#[derive(Clone)]
pub(crate) enum PointKind {
    /// A frame-replacing tail call. It dies at the next block boundary unless a
    /// merge inherits it, and admits every region it does not exempt.
    TailCall,
    /// A `break`, kept exactly while the labeled block it left is open.
    Break(BlockId),
    /// The end of an arm of a tail call's branch operand. It admits only the
    /// regions the frame held before the operand ran and the call receives on a
    /// sibling arm alone, and lives as long as the call's own point does.
    OperandArm(FxHashSet<Region>),
}

impl TailExitHoist {
    /// Whether a release of the merged root `root` may take a copy here.
    pub(in crate::lir::lower) fn admits(&self, root: Region) -> bool {
        match &self.kind {
            PointKind::OperandArm(admit) => admit.contains(&root),
            PointKind::TailCall | PointKind::Break(_) => !self.exempt.contains(&root),
        }
    }

    /// The labeled block a `break`'s point left through, which bounds its life.
    pub(in crate::lir::lower) fn left_block(&self) -> Option<BlockId> {
        match self.kind {
            PointKind::Break(id) => Some(id),
            PointKind::TailCall | PointKind::OperandArm(_) => None,
        }
    }
}

/// Where an arm of a tail call's branch operand ended, recorded as the arm
/// closes so that the call can open a point there.
pub(crate) struct OperandArmEnd {
    /// The arm's body, by which the call claims its own arms.
    arm: HirId,
    /// The finished block the arm ended in.
    block: usize,
    /// The end of that block's instruction list, ahead of its jump to the merge.
    at: usize,
    /// The regions the arm's value names, read as an operand's value is.
    names: FxHashSet<Region>,
}

impl<'a> Lowerer<'a> {
    /// The arms in value position of the tail-call operand `h`: each arm of an
    /// `if`, a `cond` or a `match`, reached through a `let` or `letrec` body and
    /// a `begin`'s tail. A short-circuit operand is no such arm, because the
    /// path past its end runs the next operand too.
    pub(in crate::lir::lower) fn operand_arms(h: &Hir, out: &mut Vec<HirId>) {
        let arm = |body: &Hir, out: &mut Vec<HirId>| {
            out.push(body.id);
            Self::operand_arms(body, out);
        };
        match &h.kind {
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::operand_arms(body, out);
            }
            HirKind::Begin(exprs) => {
                if let Some(last) = exprs.last() {
                    Self::operand_arms(last, out);
                }
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                arm(then_branch, out);
                arm(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (_, body) in clauses {
                    arm(body, out);
                }
                if let Some(e) = else_branch {
                    arm(e, out);
                }
            }
            HirKind::Match { arms, .. } => {
                for (_, _, body) in arms {
                    arm(body, out);
                }
            }
            _ => {}
        }
    }

    /// Close a branch arm whose value is `value`: store it into the branch's
    /// result slot, jump to the merge, and seal the arm's points. `body` is the
    /// arm's HIR, absent for the nil of a `cond` with no else. An arm of a tail
    /// call's branch operand records where it ended.
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
        if let Some(body) = body.filter(|b| self.tail_operand_arms.contains(&b.id)) {
            let mut names = FxHashSet::default();
            self.region_info.operand_value_regions(body, &mut names);
            let end = OperandArmEnd {
                arm: body.id,
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

    /// Open a point at the end of each recorded arm of `args`' branch
    /// operands, beside the call's own point, which is the last one pushed.
    ///
    /// An arm of argument `i` admits a region the call exempts through `i`
    /// alone, which the arm's own value does not name. A region that a later
    /// argument or the callee names anywhere in its evaluation stays out: the
    /// arm runs before them, and the stamp guards only the slot the release
    /// reads. A region that argument `i` allocates stays out too: the frame holds
    /// it only on the arm that builds it, and a sibling arm never wrote its slot.
    /// `fixed` is what the call exempts whatever its arguments name.
    pub(in crate::lir::lower) fn open_operand_arm_points(
        &mut self,
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
            let mut arms = Vec::new();
            Self::operand_arms(&args[i].expr, &mut arms);
            let mut ends = Vec::new();
            self.operand_arm_ends.retain(|e| {
                let mine = arms.contains(&e.arm);
                if mine {
                    ends.push((e.block, e.at, e.names.clone()));
                }
                !mine
            });
            let mut minted = FxHashSet::default();
            self.region_info
                .allocated_regions(&args[i].expr, &mut minted);
            let only_here: FxHashSet<Region> = values[i]
                .iter()
                .copied()
                .filter(|r| call.exempt.contains(r) && !fixed.contains(r))
                .filter(|r| !later.contains(r) && !minted.contains(r))
                .filter(|r| (0..args.len()).all(|j| j == i || !values[j].contains(r)))
                .collect();
            for (block, at, names) in ends {
                let admit: FxHashSet<Region> = only_here.difference(&names).copied().collect();
                if !admit.is_empty() {
                    self.tail_exit_hoist.push(TailExitHoist {
                        at,
                        block: HoistBlock::Finished(block),
                        operand_locals: call.operand_locals.clone(),
                        operand_captures: call.operand_captures.clone(),
                        exempt: FxHashSet::default(),
                        kind: PointKind::OperandArm(admit),
                    });
                }
            }
            self.region_info
                .evaluation_regions(&args[i].expr, &mut later);
        }
    }
}
