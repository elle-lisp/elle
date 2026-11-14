// audited: 2026-11-13
//! Placing a release so every path runs it once: a move ahead of a tail call,
//! or a replica in each closed block that leaves before reaching it.
//!
//! docs/impl/region/replicate.md
//! docs/impl/lir.md

use super::*;

mod postpone;
mod run;

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
    /// Neither placement waives the obligations. On a closure path the release
    /// fires where none did before, and escape supplies the count argument for
    /// that. No premise about instruction placement can supply it: a value the
    /// tail callee reaches through its captured environment is named by no
    /// argument and by no callee region, yet the call reads it.
    /// `frame_held_regions` is that argument. It covers the region the CALLEE
    /// hands back too, because a callee reaches a value this frame owns as an
    /// operand or through its captured environment, and by no other route
    /// (docs/impl/region/relocate.md).
    ///
    /// `TailExitHoist::exempt` and [`Self::hoistable_run`] are the two readings
    /// of what each call itself names. Both are needed, because ANF is free to
    /// rewrite how an operand is spelled. They are asked per point, so one arm's
    /// ownership move does not hold back its siblings.
    ///
    /// The emitted sequences are all stack-neutral, since each `LoadLocal` push
    /// is consumed by the release that follows it. So splicing them between the
    /// pushed arguments and the call leaves the tail call's operand layout intact.
    pub(super) fn with_tail_exit_hoist(
        &mut self,
        region: crate::hir::region::Region,
        mut f: impl FnMut(&mut Self),
    ) {
        let root = self.region_info.merged_root(region);
        // One admission, asked of the region: the frame holds it alone for as long
        // as the frame lives. The caller reads a region the callee hands BACK
        // afterwards, through the reference the tail callee's `Return` mints
        // after this release would run. That region needs no edge of its own. The
        // callee reaches a value this frame owns as an operand, where the release
        // stays behind as the ownership move, or through its captured
        // environment, a counted edge. It reaches the value by no other route.
        if self.tail_exit_hoist.is_empty() || !self.region_info.frame_held_regions.contains(&root) {
            f(self);
            return;
        }
        let admitted = |h: &super::TailExitHoist| h.admits(root);
        // A tail call emitted into this block dominates every position after it,
        // so `open_tail_exit_hoist` drops the merge's points in favour of its own
        // and its operand arms', which only ever take a replica.
        debug_assert!(
            self.tail_exit_hoist.iter().skip(1).all(|h| {
                matches!(h.kind, super::PointKind::OperandArm { .. })
                    || !matches!(self.tail_exit_hoist[0].block, super::HoistBlock::Current(_))
            }),
            "a relocation point in the open block must come first, beside its operand arms"
        );
        if let super::HoistBlock::Current(label) = self.tail_exit_hoist[0].block {
            // A tail call in this very block dominates everything after it, so
            // the placement is the move.
            let here = label == self.built().open_label();
            if here && admitted(&self.tail_exit_hoist[0]) {
                let at = self.tail_exit_hoist[0].at;
                let start = self.built().open_len();
                f(self);
                let moved = self.built().open_len() - start;
                // A run that may not move stays where `f` emitted it.
                if moved == 0
                    || !self.hoistable_run(0, start)
                    || self.move_frees_a_cell_the_window_reads(at, start)
                {
                    return;
                }
                self.tail_exit_hoist[0].at += moved;
                self.b().move_run(start, at);
                return;
            }
            // A release the call exempts may still take a replica in the arms
            // of its operands, below.
            if !here || self.tail_exit_hoist.len() == 1 {
                f(self);
                return;
            }
        }
        // Points inherited from the arms of a branch. The release stays here for
        // the paths that fall through to this merge, and a replica goes ahead of
        // each arm's tail call for the paths that leave through it.
        //
        // Whether any point actually takes a copy decides the ROUTE the release is
        // emitted with. A region every point exempts, the merged arena riding the
        // deferred slot, keeps the default release by id, so the two mechanisms
        // stay disjoint (docs/impl/region/replicate.md). An operand arm that
        // postpones the release copies the slot the release reads, so it decides
        // the route too.
        let replicates = self.tail_exit_hoist.iter().any(|h| {
            matches!(h.block, super::HoistBlock::Finished(_))
                && (admitted(h) || h.postpones(root).is_some())
        });
        let saved = std::mem::replace(&mut self.replicating_release, replicates);
        let start = self.built().open_len();
        f(self);
        let Some(route) = self.self_cancelling_run(start) else {
            self.replicating_release = saved;
            return;
        };
        for i in 0..self.tail_exit_hoist.len() {
            let super::HoistBlock::Finished(block) = self.tail_exit_hoist[i].block else {
                continue;
            };
            if !admitted(&self.tail_exit_hoist[i]) {
                continue;
            }
            // Re-run rather than clone the emitted run: each replica then names
            // its own registers, so no register is defined twice.
            let mark = self.built().mark();
            let start = self.built().open_len();
            f(self);
            let copied = self.built().open_len() - start;
            if !self.hoistable_run(i, start) || self.self_cancelling_run(start).is_none() {
                // A replica that may not go is taken back whole, with the
                // table entries only it names.
                self.b().retract(mark);
                continue;
            }
            let at = self.tail_exit_hoist[i].at;
            // A point's `at` names the position it splices at: its `TailCall`, or
            // the end of the list for a break or an operand arm, whose jump is
            // the terminator. Each replica advances it past what it inserted. So
            // the invariant is that ONE record of each point exists
            // (`begin_branch_arms` moves them rather than copying). A second
            // record keeps a stale index. It splices into the middle of the run
            // the live one already put there, and misshapes the operand stack of
            // a block that still passes every other check. Asserting the index
            // still names that position turns that into a panic here rather than
            // an out-of-bounds stack read at runtime.
            debug_assert!(
                if matches!(
                    self.tail_exit_hoist[i].kind,
                    super::PointKind::TailCall { .. }
                ) {
                    matches!(
                        self.built().finished_instr(block, at),
                        Some(InstrRef::TailCall { .. } | InstrRef::TailCallArrayMut { .. })
                    )
                } else {
                    at == self.built().finished_block_len(block)
                },
                "a relocation point's index must still name its exit position: \
                 block={block} at={at}"
            );
            self.tail_exit_hoist[i].at += copied;
            self.b().move_run_to(start, block, at);
        }
        self.postpone_arm_releases(root, route);
        self.replicating_release = saved;
    }
}
