// audited: 2026-11-12
//! The relocation points that say which paths a release still has to cover.
//! A frame-replacing tail call opens one, as do a `break` and each arm of a
//! branch operand; a branch merge inherits them.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/replicate.md

use super::*;

mod armpoint;
mod breakpoint;
pub(crate) use armpoint::{OperandArmEnd, PointKind};

/// A position some path leaves the release stream at, into which a release
/// emitted later may be MOVED or REPLICATED.
///
/// Three openers, which [`PointKind`] names. A frame-replacing tail call opens
/// one in its own block, and a `break` one at the end of the block it leaves.
/// The tail call also opens one at the end of each arm of a branch operand. A
/// branch merge inherits the points of the arms that reach it and the points that
/// already covered the branch's entry, so a point outlives its own block.
///
/// docs/impl/region/replicate.md
#[derive(Clone)]
pub(crate) struct TailExitHoist {
    /// Where in the block's instruction list a replica is spliced. That is the
    /// index of the `TailCall` for a call's point. For a break's or an operand
    /// arm's, it is the end of the list, whose jump is the block's terminator.
    /// Each splice advances the index past
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
    /// and for a break's point those the value it carries names. An operand
    /// arm's point leaves it empty, and its [`PointKind`] says what it admits.
    pub(super) exempt: rustc_hash::FxHashSet<crate::hir::region::Region>,
    /// What opened the point, which decides its lifetime and what it admits.
    pub(super) kind: PointKind,
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
    /// A branch arm the builder has already finished, by index. Blocks are only
    /// ever appended, so the index stays valid for the function's life.
    Finished(usize),
}

impl<'a> Lowerer<'a> {
    /// Start collecting the relocation points of a branch's arms. The answer is
    /// what [`Self::open_branch_merge`] needs to hand the merge block: the
    /// enclosing branch's collection to restore, and the points already covering
    /// the branch's entry.
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
        let index = self.built().finished_len();
        let label = self.built().open_label();
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
    /// `TailCall` was just emitted as the last instruction of the open block,
    /// so every release the lowerer emits after it runs on the native
    /// fall-through alone.
    ///
    /// `exempt` is read off the call itself: the regions the callee, an
    /// operand's own VALUE, the call's result placeholder, or a deferred
    /// channel name. The call then opens the points of its operands' arms
    /// ([`Self::open_operand_arm_points`]).
    ///
    /// docs/impl/region/relocate.md
    pub(super) fn open_tail_exit_hoist(
        &mut self,
        call_id: HirId,
        func: &Hir,
        args: &[crate::hir::CallArg],
        operands: &[Reg],
    ) {
        // Which slots the operands now on the stack came from, read off the
        // emitted instructions rather than the HIR. ANF may have bound an operand
        // to a synthetic binding whose region the syntax walk below does not
        // connect back to this call. The load that put it on the stack is right
        // here either way.
        let (operand_locals, operand_captures) = self.loaded_from(|r| operands.contains(&r));
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
        let fixed = exempt.clone();
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
            at: self.built().open_len() - 1,
            block: super::HoistBlock::Current(self.built().open_label()),
            operand_locals,
            operand_captures,
            exempt,
            kind: PointKind::TailCall,
        });
        self.open_operand_arm_points(func, args, &fixed);
    }

    /// The local slots and capture indices the open block's loads into
    /// registers `wanted` selects read from — what an operand now on the stack
    /// was loaded from, read off the emitted instructions.
    fn loaded_from(
        &self,
        wanted: impl Fn(Reg) -> bool,
    ) -> (rustc_hash::FxHashSet<u16>, rustc_hash::FxHashSet<u16>) {
        let mut locals = rustc_hash::FxHashSet::default();
        let mut captures = rustc_hash::FxHashSet::default();
        for i in self.built().open_instrs(0) {
            match i {
                InstrRef::LoadLocal { dst, slot } if wanted(dst) => {
                    locals.insert(slot);
                }
                InstrRef::LoadCapture { dst, index } | InstrRef::LoadCaptureRaw { dst, index }
                    if wanted(dst) =>
                {
                    captures.insert(index);
                }
                _ => {}
            }
        }
        (locals, captures)
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
    /// Two kinds of argument are reconsidered: a destructured leaf, and a
    /// retained alias (`borrowed_alias_bindings`), which the call mints a
    /// reference of its own and so moves none of the frame's. Every other
    /// binding that names a region names the whole value. A region a leaf's
    /// rest name HOLDS keeps its exemption outright. Each remaining one keeps it
    /// where the call passes the very slot that region's release route loads. A
    /// region with no recorded slot releases by id and keeps it too, there being
    /// no slot to compare.
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
        if !self.region_info.destructure_leaf_bindings.contains(b)
            && !self.borrowed_alias_bindings.contains(b)
        {
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
            if self.region_info.names_built_rest_collection(*b, root) {
                continue;
            }
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
