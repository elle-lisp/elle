// audited: 2026-09-05
//! The relocation points that say which paths a release still has to cover.
//! A frame-replacing tail call opens one, and a branch merge inherits them.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/replicate.md

use super::*;

/// The relocation point a frame-replacing tail call opens in its own block
/// (docs/impl/region/relocate.md).
///
/// Everything the lowerer emits after a `TailCall` runs only on the NATIVE
/// fall-through — a native pushes no bytecode frame, so the dispatch loop
/// continues into that block, while a closure callee replaces the frame and
/// never arrives. A release landing there is therefore emitted where control may
/// never reach, and the frame's own reference is stranded once per call. Moving
/// that one release to just before the `TailCall` costs no count argument (it is
/// the same single release, relocated), and is legal for every region the call
/// itself cannot reach.
///
/// A branch merge inherits the points of the arms that reach it AND the points
/// that already covered the branch's entry, so a point outlives its own block
/// (docs/impl/region/replicate.md). There the release is emitted at
/// the merge *and* replicated at each point, which is sound only for a
/// self-cancelling run — one that nil-stamps the slot it read, so the copy a path
/// reaches second no-ops.
#[derive(Clone)]
pub(crate) struct TailExitHoist {
    /// Index of the `TailCall` in its block's instruction list. A hoisted
    /// release is spliced in here, ahead of the frame replacement, and the index
    /// advances so successive hoists keep their emission order.
    pub(super) at: usize,
    /// The block the `TailCall` sits in.
    pub(super) block: HoistBlock,
    /// The local slots and capture indices the call's operands were loaded from.
    /// A release that reloads one of these reloads the very value now sitting on
    /// the operand stack, so it IS the ownership move however ANF spelled the
    /// argument — the reading `exempt` cannot give, since ANF is free to rewrite
    /// an operand into a synthetic binding whose region the syntax walk does not
    /// connect back to the call.
    pub(super) operand_locals: rustc_hash::FxHashSet<u16>,
    pub(super) operand_captures: rustc_hash::FxHashSet<u16>,
    /// Regions the callee or an argument subtree names, canonicalized through
    /// the merge forest. These releases must STAY in the dead block: an
    /// argument's is the ownership move the calling convention rests on, and the
    /// callee's belongs to the activation that takes it over.
    pub(super) exempt: rustc_hash::FxHashSet<crate::hir::region::Region>,
}

/// What a branch lowering holds across its arms, so `open_branch_merge` can hand
/// the merge block everything that covers it.
///
/// Two sources, collected at different moments because they are sealed
/// differently: an arm's own points name a block the arm just closed and are
/// sealed at that close (`seal_arm_hoists`), while the points covering the
/// branch's ENTRY are already sealed when the branch begins and are read there
/// (docs/impl/region/replicate.md).
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
    /// The three branch lowerings bracket their arms with this pair; a branch
    /// nested inside an arm therefore collects into its own list and hands its
    /// union up as that arm's contribution.
    ///
    /// The **inherited** half is the merge's second source. A merge is reached
    /// only through the branch, so the paths that arrive at it are the paths that
    /// arrived at the entry, minus the ones an arm's own tail call took away — and
    /// a point that covered the entry covers the merge for the same reason it
    /// covered the entry (docs/impl/region/replicate.md). Without it a branch that follows an
    /// earlier branch starts life covering nothing: the condition block closes like
    /// any other and clears what it was carrying.
    ///
    /// Read here rather than sealed later, because this runs while the branch's own
    /// entry block is the one still open — the position the points describe. Only
    /// the [`super::HoistBlock::Finished`] ones are taken: a point still naming the
    /// open block dies with that block exactly as before, having no closed
    /// instruction list to be spliced into.
    ///
    /// The points are MOVED out rather than copied, so the branch holds the only
    /// record of each. A point is mutable state — every replica spliced into its
    /// block advances its `at` past what it inserted — so two records of one point
    /// diverge, and the stale one then splices into the middle of the run the
    /// current one already put there. What that costs is the position between here
    /// and the branch's own first block boundary: a `cond`'s first clause test and a
    /// `match`'s first pattern test are lowered into the entry block and emit their
    /// releases plainly. That is the conservative baseline this whole mechanism
    /// improves on, never a mis-free.
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

    /// Hand the merge block just opened the points its arms sealed and the points
    /// that covered the branch's entry, and restore the enclosing branch's
    /// collection.
    ///
    /// Every path into a merge arrives through one of the arms, and every path
    /// into the branch arrived at its entry, so the two sets together cover the
    /// merge — which is what licenses replicating a release emitted here back into
    /// each of them. Neither can double-count the other: a point handed over from
    /// the entry was never in an arm's `tail_exit_hoist` to be sealed, the block
    /// boundary between the two having cleared it.
    pub(super) fn open_branch_merge(&mut self, hoists: super::BranchHoists) {
        let super::BranchHoists { saved, inherited } = hoists;
        self.tail_exit_hoist = std::mem::replace(&mut self.arm_exit_hoists, saved);
        self.tail_exit_hoist.extend(inherited);
    }

    /// Open the relocation point a frame-replacing tail call leaves behind: the
    /// `TailCall` was just emitted as the last instruction of `current_block`,
    /// and every release the lowerer emits after it into this block runs only on
    /// the native fall-through (docs/impl/region/relocate.md).
    ///
    /// `exempt` is read off the call itself — the regions the callee, an operand's
    /// own VALUE, or the call's own result placeholder name — because those are
    /// exactly the ones the tail call can still reach, and their releases are
    /// owed to the ownership move, to the activation that takes over the callee's
    /// region, and to the caller that consumes the result.
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
        // deferred release (`TailCall::deferred_release_slot`,
        // docs/impl/region/letrec.md): its binding-scope `DecrefRegion` is dead
        // past the frame replacement BY DESIGN, and the deferred channel supplies
        // it. Hoisting it would make both fire.
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
        // covers them — any points a merge left here name arms that reach this
        // call, not the releases that follow it.
        self.tail_exit_hoist.clear();
        self.tail_exit_hoist.push(super::TailExitHoist {
            at: self.current_block.instructions.len() - 1,
            block: super::HoistBlock::Current(self.current_block.label),
            operand_locals,
            operand_captures,
            exempt,
        });
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

    /// Take back the exemption of a region an argument only NAMES.
    ///
    /// An argument's region is exempt because the callee's owned-parameter
    /// release stands in for the caller's — which holds only where the reference
    /// the callee takes over is the one the caller's release would have dropped.
    /// A destructured leaf is where the two come apart: `(let [[a b] t] (f a b))`
    /// hands `f` the leaves, never `t`, yet each leaf names `t`'s region through
    /// `binding_source_regions` (a leaf may BE an element living in it). So `t`'s
    /// release is withheld, nothing takes it over, and `t` is held to fiber
    /// teardown — one region per call, which every h2 frame builder pays.
    ///
    /// Only a leaf is reconsidered (`RegionInfo::destructure_leaf_bindings`).
    /// Every other binding that names a region names the whole value: an alias
    /// binder is a second name for the very reference the call moves — `arrs` for
    /// the array `a` built and returned by an inner `let` (stdlib `zip`) — and
    /// hoisting its release ahead of the call would free what the callee is about
    /// to take over.
    ///
    /// The slot is the second half of the reading, and it is what admits the leaf
    /// that IS the whole: a rest pattern binds a tail that shares the source's
    /// region and can be the reference the call moves. Where the region's value
    /// route loads a slot the call passes, the move is real and the exemption
    /// stands whatever the binding's kind. A region with no recorded slot releases
    /// by id, where there is no slot to compare, and stands too.
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
