// audited: 2026-09-15
//! The relocation points that say which paths a release still has to cover.
//! A frame-replacing tail call opens one and a `break` opens one; a branch merge
//! inherits them.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/replicate.md

use super::*;

/// A position some path leaves the release stream at, into which a release
/// emitted later may be MOVED or REPLICATED.
///
/// Two openers: a frame-replacing tail call opens one in its own block, and a
/// `break` opens one at the end of the block it leaves. A branch merge inherits
/// the points of the arms that reach it and the points that already covered the
/// branch's entry, so a point outlives its own block.
///
/// docs/impl/region/replicate.md
#[derive(Clone)]
pub(crate) struct TailExitHoist {
    /// Where in the block's instruction list a replica is spliced: the index of
    /// the `TailCall` for a call's point, and the end of the list for a break's,
    /// whose jump is the block's terminator. Each splice advances the index past
    /// what it inserted, so successive replicas keep their emission order.
    pub(super) at: usize,
    /// The block the point sits in.
    pub(super) block: HoistBlock,
    /// The local slots and capture indices the call's operands were loaded
    /// from, read off the emitted instructions rather than the HIR. A release
    /// that reloads one of these IS the ownership move, however ANF spelled the
    /// argument.
    pub(super) operand_locals: rustc_hash::FxHashSet<u16>,
    pub(super) operand_captures: rustc_hash::FxHashSet<u16>,
    /// Regions whose release must STAY where the lowerer put it, canonicalized
    /// through the merge forest: those the callee or an argument subtree names,
    /// and for a break's point those the value it carries names.
    pub(super) exempt: rustc_hash::FxHashSet<crate::hir::region::Region>,
    /// The labeled block a `break` left through, and the point's whole lifetime:
    /// it is kept exactly while that block is open. `None` for a tail call's
    /// point, which dies at the next block boundary unless a merge inherits it.
    pub(super) left_block: Option<BlockId>,
}

/// What a branch lowering holds across its arms, so `open_branch_merge` can
/// hand the merge block everything that covers it. Two sources, collected at
/// different moments because they are sealed differently.
///
/// docs/impl/region/replicate.md
pub(crate) struct BranchHoists {
    /// The enclosing branch's `arm_exit_hoists`, restored at the merge so a
    /// nested branch's arms never leak into it.
    pub(super) saved: Vec<TailExitHoist>,
    /// The points that already covered the position the branch was entered at.
    pub(super) inherited: Vec<TailExitHoist>,
}

/// Where a relocation point lives, and with it which of the two placements
/// applies: a MOVE within the block still being filled, or a REPLICA spliced
/// into an arm that has already closed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoistBlock {
    /// The block the lowerer is filling. Its label validates the point — a
    /// stale one names an instruction list this block no longer is.
    Current(Label),
    /// A branch arm already pushed onto `LirFunction::blocks`, by index. Blocks
    /// are only ever appended, so the index stays valid for the function's life.
    Finished(usize),
}

impl<'a> Lowerer<'a> {
    /// Start collecting the relocation points of a branch's arms, returning what
    /// [`Self::open_branch_merge`] needs to hand the merge block: the enclosing
    /// branch's collection to restore, and the points already covering the
    /// position this branch is entered at.
    ///
    /// Each branch lowering brackets its arms with this pair — `if`, `cond`,
    /// `match`, and the branch `and`/`or` compile to. A branch nested inside an
    /// arm collects into its own list and hands its union up.
    ///
    /// Only the [`super::HoistBlock::Finished`] points are inherited: one still
    /// naming the open block has no closed instruction list to be spliced into.
    /// They are MOVED out rather than copied, a point being mutable state that
    /// two records of would diverge.
    ///
    /// docs/impl/region/replicate.md
    pub(super) fn begin_branch_arms(&mut self) -> super::BranchHoists {
        let mut inherited = Vec::new();
        self.tail_exit_hoist.retain(|h| match h.block {
            super::HoistBlock::Finished(_) => {
                inherited.push(h.clone());
                false
            }
            super::HoistBlock::Current(_) => true,
        });
        super::BranchHoists {
            saved: std::mem::take(&mut self.arm_exit_hoists),
            inherited,
        }
    }

    /// Seal the relocation points of the arm-final block into the branch's
    /// collection. Called immediately before the arm's `finish_block`, while the
    /// block index that call is about to assign is still predictable.
    ///
    /// Rebasing here is what keeps every sealed point addressable: `blocks` is
    /// only ever appended to, so the index this arm is about to take stays valid
    /// for the rest of the function. A point naming some *other* block cannot be
    /// spliced into from here and is dropped rather than carried stale.
    pub(super) fn seal_arm_hoists(&mut self) {
        let index = self.current_func.blocks.len();
        let label = self.current_block.label;
        let sealed = self
            .tail_exit_hoist
            .drain(..)
            .filter_map(|mut h| match h.block {
                super::HoistBlock::Current(l) if l == label => {
                    h.block = super::HoistBlock::Finished(index);
                    Some(h)
                }
                super::HoistBlock::Current(_) => None,
                super::HoistBlock::Finished(_) => Some(h),
            });
        self.arm_exit_hoists.extend(sealed);
    }

    /// Hand the merge block just opened the points its arms sealed and the
    /// points that covered the branch's entry, and restore the enclosing
    /// branch's collection. The two sets cannot double-count each other. The
    /// break scope filter is asked here too, the merge being a position past
    /// the branch.
    ///
    /// docs/impl/region/replicate.md
    pub(super) fn open_branch_merge(&mut self, hoists: super::BranchHoists) {
        let super::BranchHoists { saved, inherited } = hoists;
        self.tail_exit_hoist = std::mem::replace(&mut self.arm_exit_hoists, saved);
        self.tail_exit_hoist.extend(inherited);
        self.retain_open_break_points();
    }

    /// Open the relocation point a frame-replacing tail call leaves behind. The
    /// `TailCall` was just emitted as the last instruction of `current_block`,
    /// so every release the lowerer emits after it runs on the native
    /// fall-through alone.
    ///
    /// `exempt` is read off the call itself: the regions the callee, an
    /// operand's own VALUE, the call's result placeholder, or a deferred
    /// channel name.
    ///
    /// docs/impl/region/relocate.md
    pub(super) fn open_tail_exit_hoist(
        &mut self,
        call_id: HirId,
        func: &Hir,
        args: &[crate::hir::CallArg],
        operands: &[Reg],
    ) {
        // Which slots the operands now on the stack came from. Read off the
        // emitted instructions rather than the HIR, because ANF may have bound an
        // operand to a synthetic binding whose region the syntax walk below does
        // not connect back to this call — but the load that put it on the stack
        // is right here either way.
        let mut operand_locals = rustc_hash::FxHashSet::default();
        let mut operand_captures = rustc_hash::FxHashSet::default();
        for i in &self.current_block.instructions {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } if operands.contains(dst) => {
                    operand_locals.insert(*slot);
                }
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index }
                    if operands.contains(dst) =>
                {
                    operand_captures.insert(*index);
                }
                _ => {}
            }
        }
        let mut exempt = rustc_hash::FxHashSet::default();
        if let Some(&r) = self.region_info.alloc_region.get(&call_id) {
            exempt.insert(self.region_info.merged_root(r));
        }
        // The merged arena a letrec body's tail call hands to the runtime's
        // deferred release: that channel supplies the release, so hoisting it
        // here would make both fire (docs/impl/region/letrec.md).
        if let Some(&root) = self.region_info.cycle_tail_release.get(&call_id) {
            exempt.insert(self.region_info.merged_root(root));
        }
        self.collect_operand_regions(func, &mut exempt);
        // The argument half is reconsidered per region before it joins `exempt`,
        // so a region the CALLEE or the call's own result already exempts keeps
        // that exemption whatever an argument names.
        let mut by_args = rustc_hash::FxHashSet::default();
        for a in args {
            self.collect_operand_regions(&a.expr, &mut by_args);
        }
        for a in args {
            self.drop_named_only_arg_exemptions(
                &a.expr,
                &operand_locals,
                &operand_captures,
                &mut by_args,
            );
        }
        exempt.extend(by_args);
        // This call dominates every position after it in the block, so it alone
        // covers them and every earlier point is dropped. Dropping a licence to
        // replicate can only over-keep.
        self.tail_exit_hoist.clear();
        self.tail_exit_hoist.push(super::TailExitHoist {
            at: self.current_block.instructions.len() - 1,
            block: super::HoistBlock::Current(self.current_block.label),
            operand_locals,
            operand_captures,
            exempt,
            left_block: None,
        });
    }

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
    pub(super) fn open_break_exit_hoist(&mut self, block_id: BlockId, value: &Hir, value_reg: Reg) {
        let mut operand_locals = rustc_hash::FxHashSet::default();
        let mut operand_captures = rustc_hash::FxHashSet::default();
        for i in &self.current_block.instructions {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } if *dst == value_reg => {
                    operand_locals.insert(*slot);
                }
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index }
                    if *dst == value_reg =>
                {
                    operand_captures.insert(*index);
                }
                _ => {}
            }
        }
        let mut exempt = rustc_hash::FxHashSet::default();
        self.collect_operand_regions(value, &mut exempt);
        self.tail_exit_hoist.push(super::TailExitHoist {
            at: self.current_block.instructions.len(),
            block: super::HoistBlock::Finished(self.current_func.blocks.len()),
            operand_locals,
            operand_captures,
            exempt,
            left_block: Some(block_id),
        });
    }

    /// Drop every break point whose block has finished lowering. A tail call's
    /// point has no such scope and is left alone; the block boundaries decide
    /// its life instead.
    ///
    /// docs/impl/region/replicate.md
    pub(super) fn retain_open_break_points(&mut self) {
        if self.tail_exit_hoist.iter().all(|h| h.left_block.is_none()) {
            return;
        }
        let open: Vec<BlockId> = self
            .block_lower_contexts
            .iter()
            .map(|c| c.block_id)
            .collect();
        self.tail_exit_hoist
            .retain(|h| h.left_block.is_none_or(|id| open.contains(&id)));
    }

    /// Every region one tail-call OPERAND — the callee, or an argument — may hand
    /// the call, read off [`crate::hir::region::RegionInfo::operand_value_regions`]
    /// so this exemption and the branch-arm window's per-point funding question ask
    /// one reading rather than two.
    fn collect_operand_regions(
        &self,
        h: &Hir,
        out: &mut rustc_hash::FxHashSet<crate::hir::region::Region>,
    ) {
        self.region_info.operand_value_regions(h, out);
    }

    /// Take back the exemption of a region an argument only NAMES, one region at
    /// a time.
    ///
    /// Only a destructured leaf is reconsidered; every other binding that names a
    /// region names the whole value. A region keeps its exemption where the call
    /// passes the very slot that region's release route loads. A region with no
    /// recorded slot releases by id and keeps it too, there being no slot to
    /// compare.
    ///
    /// docs/impl/region/relocate.md
    fn drop_named_only_arg_exemptions(
        &self,
        h: &Hir,
        operand_locals: &rustc_hash::FxHashSet<u16>,
        operand_captures: &rustc_hash::FxHashSet<u16>,
        exempt: &mut rustc_hash::FxHashSet<crate::hir::region::Region>,
    ) {
        let HirKind::Var(b) = &h.kind else { return };
        if !self.region_info.destructure_leaf_bindings.contains(b) {
            return;
        }
        for &r in self
            .region_info
            .binding_source_regions
            .get(b)
            .into_iter()
            .flatten()
        {
            let root = self.region_info.merged_root(r);
            let moved = match self.region_to_slot.get(&root) {
                Some(super::ValueSlot::Local(s)) => operand_locals.contains(s),
                Some(super::ValueSlot::Env(i)) => operand_captures.contains(i),
                None => true,
            };
            if !moved {
                exempt.remove(&root);
            }
        }
    }
}
