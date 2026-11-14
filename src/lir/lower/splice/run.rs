// audited: 2026-11-13
//! Which emitted release run may move ahead of a tail call or take a replica: what it reloads, what it stamps, which cell it frees.
//!
//! docs/impl/region/replicate.md
//! docs/impl/region/relocate.md
//! docs/impl/region/bindings.md

use super::super::*;

impl<'a> Lowerer<'a> {
    /// The slot this run releases by VALUE and nil-stamps, if it does both.
    ///
    /// That stamp lets one release be emitted at a merge and replicated at each
    /// relocation point. Whichever copy a path reaches first does the work and
    /// blanks the holder slot. Any later copy loads `nil`, whose release is a
    /// no-op. It is the same discipline that lets a branch's per-arm
    /// compensations coexist with its `decref_point`.
    ///
    /// Everything else keeps the baseline, and the exclusions are deliberate. A
    /// `DecrefRegion` by region id, a capture cell's `DecrefCellRegion` and the
    /// transfer `AdoptIntoActivation` all leave the holder as it was. So a second
    /// copy on one path would count twice.
    ///
    /// The run is the open block's instructions from `start` on.
    pub(in crate::lir::lower) fn self_cancelling_run(&self, start: usize) -> Option<u16> {
        let mut released: Option<u16> = None;
        let mut nil_regs: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        let mut loaded: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut stamped = false;
        for i in self.built().open_instrs(start) {
            match i {
                InstrRef::LoadLocal { dst, slot } => {
                    loaded.insert(dst, slot);
                }
                InstrRef::Const {
                    dst,
                    value: ConstRef::Nil,
                } => {
                    nil_regs.insert(dst);
                }
                InstrRef::DecrefValueRegion { src } => match loaded.get(&src) {
                    // One release, of a value this run loaded from a slot.
                    Some(slot) if released.is_none() => released = Some(*slot),
                    _ => return None,
                },
                InstrRef::StoreLocal { slot, src } => {
                    if released == Some(slot) && nil_regs.contains(&src) {
                        stamped = true;
                    }
                }
                _ => return None,
            }
        }
        released.filter(|_| stamped)
    }

    /// Would moving the run from `start` back to `at` free an env cell that the
    /// instructions it crosses still read?
    ///
    /// The two refusals of [`Self::hoistable_run`] are about the region and the
    /// call; this one is about two REGIONS. A captured binding's value and its box
    /// share one env index, and the value's `DecrefValueRegion` loads the box RAW
    /// and unwraps it — so it reads the page the box's `DecrefCellRegion` frees.
    /// The relocation answers per region and can move one of the pair alone, which
    /// inverts the order the `decref_point` clamp established
    /// (docs/impl/region/bindings.md).
    ///
    /// The window is everything from `at` to the start of the run, `start` — the
    /// `TailCall` and every release already emitted after it. Reading it is enough
    /// because the clamp fixes the emission order: a release routed through the
    /// cell is already in the window by the time the cell's own release asks to
    /// move. Declining leaves the box release on the closure path, the same bounded
    /// fallback every other refusal takes.
    ///
    /// The replica placement needs no such question: only a self-cancelling run is
    /// replicated, and a cell release is not one ([`Self::self_cancelling_run`]).
    pub(in crate::lir::lower) fn move_frees_a_cell_the_window_reads(
        &self,
        at: usize,
        start: usize,
    ) -> bool {
        let mut from_index: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut freed: rustc_hash::FxHashSet<u16> = rustc_hash::FxHashSet::default();
        for i in self.built().open_instrs(start) {
            match i {
                InstrRef::LoadCapture { dst, index } | InstrRef::LoadCaptureRaw { dst, index } => {
                    from_index.insert(dst, index);
                }
                InstrRef::DecrefCellRegion { src } => {
                    freed.extend(from_index.get(&src).copied());
                }
                _ => {}
            }
        }
        if freed.is_empty() {
            return false;
        }
        self.built().open_instrs(at).take(start - at).any(|i| {
            matches!(
                i,
                InstrRef::LoadCapture { index, .. } | InstrRef::LoadCaptureRaw { index, .. }
                    if freed.contains(&index)
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
    ///   owned-param release consumes. Running it here drops the callee's
    ///   reference (`TailExitHoist::operand_locals`).
    /// - **It reads a register defined outside the run.** The only such register a
    ///   release names is the enclosing node's just-lowered value. For a node
    ///   whose subtree ends in the tail call, that call defines it, and the hoist
    ///   point precedes the definition. (This is the discarded-result route of
    ///   `emit_decrefs_for`.)
    ///
    /// The run is the open block's instructions from `start` on.
    pub(in crate::lir::lower) fn hoistable_run(&self, index: usize, start: usize) -> bool {
        let Some(h) = self.tail_exit_hoist.get(index) else {
            return false;
        };
        let mut defined: rustc_hash::FxHashSet<Reg> = rustc_hash::FxHashSet::default();
        for i in self.built().open_instrs(start) {
            match i {
                InstrRef::LoadLocal { dst, slot } => {
                    if h.operand_locals.contains(&slot) {
                        return false;
                    }
                    defined.insert(dst);
                }
                InstrRef::LoadCapture { dst, index } | InstrRef::LoadCaptureRaw { dst, index } => {
                    if h.operand_captures.contains(&index) {
                        return false;
                    }
                    defined.insert(dst);
                }
                InstrRef::Const { dst, .. } => {
                    defined.insert(dst);
                }
                InstrRef::StoreLocal { src, .. }
                | InstrRef::DecrefValueRegion { src }
                | InstrRef::DecrefCellRegion { src }
                | InstrRef::AdoptIntoActivation { child: src } => {
                    if !defined.contains(&src) {
                        return false;
                    }
                }
                InstrRef::DecrefRegion { .. } => {}
                // Anything else is outside the release vocabulary this wrapper
                // was written for; leave it where the lowerer put it.
                _ => return false,
            }
        }
        true
    }
}
