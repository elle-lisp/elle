// audited: 2026-09-05
//! Placing a release so every path runs it once: a move ahead of a tail call,
//! or a replica in each closed block that leaves before reaching it.
//!
//! docs/impl/region/replicate.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Emit `f`'s instructions for `region`, placed so that every path runs the
    /// release exactly once even where a frame-replacing tail call stands between
    /// this position and the paths that reach it.
    ///
    /// Two placements, decided by where the relocation points sit:
    ///
    /// - **A point in this block** — the tail call was emitted here, so the run is
    ///   MOVED ahead of it. The same single instruction sequence, at one position,
    ///   reached by the closure path and the native fall-through alike.
    /// - **Points a merge inherited from its arms** — those blocks are closed, so
    ///   the run is emitted here AND replicated at each point. This counts once per
    ///   path only because a value-routed release nil-stamps the slot it read
    ///   ([`Self::self_cancelling_run`]); a run without that stamp keeps the
    ///   baseline.
    ///
    /// Neither placement waives the obligations. Escape supplies the count
    /// argument — on a closure path the release fires where none did before, and no
    /// premise about instruction placement can supply it: a value the tail callee
    /// reaches through its captured environment is named by no argument and by no
    /// callee region, yet the call reads it. `frame_held_regions` is that argument,
    /// and it covers the region the CALLEE hands back too, because a callee reaches
    /// a value this frame owns as an operand or through its captured environment and
    /// by no other route (docs/impl/region/relocate.md). `TailExitHoist::exempt` and
    /// [`Self::hoistable_run`] are the two readings of what each call itself names
    /// — both needed, because ANF is free to rewrite how an operand is spelled —
    /// and they are asked per point, so one arm's ownership move does not hold back
    /// its siblings.
    ///
    /// The emitted sequences are all stack-neutral — each `LoadLocal` push is
    /// consumed by the release that follows it — so splicing them between the
    /// pushed arguments and the call leaves the tail call's operand layout intact.
    pub(super) fn with_tail_exit_hoist(
        &mut self,
        region: crate::hir::region::Region,
        mut f: impl FnMut(&mut Self),
    ) {
        let root = self.region_info.merged_root(region);
        // One admission, asked of the region: the frame holds it alone for as long
        // as the frame lives. A region the callee hands BACK is read afterwards —
        // by the caller, through the reference the tail callee's `Return` mints
        // after this release would run — and needs no edge of its own, because the
        // callee reaches a value this frame owns as an operand (where the release
        // stays behind as the ownership move) or through its captured environment
        // (a counted edge) and by no other route.
        if self.tail_exit_hoist.is_empty() || !self.region_info.frame_held_regions.contains(&root) {
            f(self);
            return;
        }
        let admitted = |h: &super::TailExitHoist| !h.exempt.contains(&root);
        // The two placements never mix: a tail call emitted into this block
        // dominates every position after it, so `open_tail_exit_hoist` drops the
        // merge's points in favour of its own single one.
        debug_assert!(
            self.tail_exit_hoist.len() == 1
                || !self
                    .tail_exit_hoist
                    .iter()
                    .any(|h| matches!(h.block, super::HoistBlock::Current(_))),
            "a relocation point in the open block must be the only one"
        );
        if let super::HoistBlock::Current(label) = self.tail_exit_hoist[0].block {
            // A tail call in this very block dominates everything after it, so it
            // is the only point and the placement is the move.
            if label != self.current_block.label || !admitted(&self.tail_exit_hoist[0]) {
                f(self);
                return;
            }
            let at = self.tail_exit_hoist[0].at;
            let start = self.current_block.instructions.len();
            f(self);
            let moved: Vec<_> = self.current_block.instructions.drain(start..).collect();
            if moved.is_empty()
                || !self.hoistable_run(0, &moved)
                || self.move_frees_a_cell_the_window_reads(at, &moved)
            {
                self.current_block.instructions.extend(moved);
                return;
            }
            self.tail_exit_hoist[0].at += moved.len();
            self.current_block.instructions.splice(at..at, moved);
            return;
        }
        // Points inherited from the arms of a branch. The release stays here for
        // the paths that fall through to this merge, and a replica goes ahead of
        // each arm's tail call for the paths that leave through it.
        //
        // Whether any point actually takes a copy decides the ROUTE the release is
        // emitted with: a region every point exempts — the merged arena riding the
        // deferred slot — keeps the default release by id, so the two mechanisms
        // stay disjoint (docs/impl/region/replicate.md).
        let replicates = self
            .tail_exit_hoist
            .iter()
            .any(|h| matches!(h.block, super::HoistBlock::Finished(_)) && admitted(h));
        let saved = std::mem::replace(&mut self.replicating_release, replicates);
        let start = self.current_block.instructions.len();
        f(self);
        if !self.self_cancelling_run(&self.current_block.instructions[start..]) {
            self.replicating_release = saved;
            return;
        }
        for i in 0..self.tail_exit_hoist.len() {
            let super::HoistBlock::Finished(block) = self.tail_exit_hoist[i].block else {
                continue;
            };
            if !admitted(&self.tail_exit_hoist[i]) {
                continue;
            }
            // Re-run rather than clone the emitted run: each replica then names
            // its own registers, so no register is defined twice.
            let start = self.current_block.instructions.len();
            f(self);
            let copy: Vec<_> = self.current_block.instructions.drain(start..).collect();
            if !self.hoistable_run(i, &copy) || !self.self_cancelling_run(&copy) {
                continue;
            }
            let at = self.tail_exit_hoist[i].at;
            // A point's `at` names the position it splices at — its `TailCall`,
            // or the end of the list for a break, whose jump is the terminator —
            // and each replica advances it past what it inserted. So the
            // invariant is that ONE record of each point exists
            // (`begin_branch_arms` moves them rather than copying). A second
            // record keeps a stale index, which splices into the middle of the
            // run the live one already put there and leaves the operand stack of
            // a block that still passes every other check misshapen. Asserting
            // the index still names that position turns it into a panic here
            // rather than an out-of-bounds stack read at runtime.
            debug_assert!(
                if self.tail_exit_hoist[i].left_block.is_some() {
                    at == self.current_func.blocks[block].instructions.len()
                } else {
                    matches!(
                        self.current_func.blocks[block].instructions.get(at),
                        Some(SpannedInstr {
                            instr: LirInstr::TailCall { .. } | LirInstr::TailCallArrayMut { .. },
                            ..
                        })
                    )
                },
                "a relocation point's index must still name its exit position: \
                 block={block} at={at}"
            );
            self.tail_exit_hoist[i].at += copy.len();
            self.current_func.blocks[block]
                .instructions
                .splice(at..at, copy);
        }
        self.replicating_release = saved;
    }

    /// Does this run release by VALUE and nil-stamp the slot it read?
    ///
    /// That stamp is what lets one release be emitted at a merge and replicated
    /// ahead of a branch arm's tail call: whichever copy a path reaches first does
    /// the work and blanks the holder slot, and any later copy loads `nil`, whose
    /// release is a no-op. It is the same discipline that lets a branch's per-arm
    /// compensations coexist with its `decref_point`.
    ///
    /// Everything else keeps the baseline, and the exclusions are the point rather
    /// than an accident of the vocabulary: a `DecrefRegion` by region id, a capture
    /// cell's `DecrefCellRegion` and the transfer `AdoptIntoActivation` all leave
    /// the holder as it was, so a second copy on one path would count twice.
    fn self_cancelling_run(&self, run: &[SpannedInstr]) -> bool {
        let mut released: Option<u16> = None;
        let mut nil_regs: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        let mut loaded: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut stamped = false;
        for i in run {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    loaded.insert(*dst, *slot);
                }
                LirInstr::Const {
                    dst,
                    value: LirConst::Nil,
                } => {
                    nil_regs.insert(*dst);
                }
                LirInstr::DecrefValueRegion { src } => match loaded.get(src) {
                    // One release, of a value this run loaded from a slot.
                    Some(slot) if released.is_none() => released = Some(*slot),
                    _ => return false,
                },
                LirInstr::StoreLocal { slot, src } => {
                    if released == Some(*slot) && nil_regs.contains(src) {
                        stamped = true;
                    }
                }
                _ => return false,
            }
        }
        stamped
    }

    /// Would moving `run` back to `at` free an env cell that the instructions it
    /// crosses still read?
    ///
    /// The two refusals of [`Self::hoistable_run`] are about the region and the
    /// call; this one is about two REGIONS. A captured binding's value and its box
    /// share one env index, and the value's `DecrefValueRegion` loads the box RAW
    /// and unwraps it — so it reads the page the box's `DecrefCellRegion` frees.
    /// The relocation answers per region and can move one of the pair alone, which
    /// inverts the order the `decref_point` clamp established
    /// (docs/impl/region/bindings.md).
    ///
    /// The window is everything from `at` to the end of the open block — the
    /// `TailCall` and every release already emitted after it. Reading it is enough
    /// because the clamp fixes the emission order: a release routed through the
    /// cell is already in the window by the time the cell's own release asks to
    /// move. Declining leaves the box release on the closure path, the same bounded
    /// fallback every other refusal takes.
    ///
    /// The replica placement needs no such question: only a self-cancelling run is
    /// replicated, and a cell release is not one ([`Self::self_cancelling_run`]).
    fn move_frees_a_cell_the_window_reads(&self, at: usize, run: &[SpannedInstr]) -> bool {
        let mut from_index: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut freed: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
        for i in run {
            match &i.instr {
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index } => {
                    from_index.insert(*dst, *index);
                }
                LirInstr::DecrefCellRegion { src } => {
                    freed.extend(from_index.get(src).copied());
                }
                _ => {}
            }
        }
        if freed.is_empty() {
            return false;
        }
        self.current_block.instructions[at..].iter().any(|i| {
            matches!(
                &i.instr,
                LirInstr::LoadCapture { index, .. } | LirInstr::LoadCaptureRaw { index, .. }
                    if freed.contains(index)
            )
        })
    }

    /// May this just-emitted release run ahead of the tail call at point `index`?
    ///
    /// Two refusals, both about what the instructions themselves name rather than
    /// what the HIR said:
    ///
    /// - **It reloads an operand's slot.** The value now on the operand stack came
    ///   from that slot, so this release is the ownership move the callee's
    ///   owned-param release consumes — running it here drops the callee's
    ///   reference (`TailExitHoist::operand_locals`).
    /// - **It reads a register defined outside the run.** The only such register a
    ///   release names is the enclosing node's just-lowered value, which for a node
    ///   whose subtree ends in the tail call is produced BY that call — a
    ///   definition the hoist point precedes. (This is the discarded-result route
    ///   of `emit_decrefs_for`.)
    fn hoistable_run(&self, index: usize, run: &[SpannedInstr]) -> bool {
        let Some(h) = self.tail_exit_hoist.get(index) else {
            return false;
        };
        let mut defined: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        for i in run {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    if h.operand_locals.contains(slot) {
                        return false;
                    }
                    defined.insert(*dst);
                }
                LirInstr::LoadCapture { dst, index } | LirInstr::LoadCaptureRaw { dst, index } => {
                    if h.operand_captures.contains(index) {
                        return false;
                    }
                    defined.insert(*dst);
                }
                LirInstr::Const { dst, .. } => {
                    defined.insert(*dst);
                }
                LirInstr::StoreLocal { src, .. }
                | LirInstr::DecrefValueRegion { src }
                | LirInstr::DecrefCellRegion { src }
                | LirInstr::AdoptIntoActivation { child: src } => {
                    if !defined.contains(src) {
                        return false;
                    }
                }
                LirInstr::DecrefRegion { .. } => {}
                // Anything else is outside the release vocabulary this wrapper
                // was written for; leave it where the lowerer put it.
                _ => return false,
            }
        }
        true
    }
}
