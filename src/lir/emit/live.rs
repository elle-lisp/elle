// audited: 2026-11-05
// src/lir/AGENTS.md
//! The registers live into each block: those some path from the block's entry reads before it writes them.
//!
//! An edge pops a cell only when its target never reads the register the cell
//! holds. The simulation's register map cannot answer that: once a `DupN` copy
//! is consumed it names no cell for the register, though the original cell may
//! still hold a value a later block reads.

use super::super::code::LirView;
use super::super::types::{for_each_terminator_use, Label, Reg, Terminator};
use rustc_hash::FxHashMap;
use std::cmp::Ordering;

/// A set of registers, one bit per register id.
#[derive(Clone, Debug, Default)]
pub(super) struct RegSet(Vec<u64>);

impl RegSet {
    pub(super) fn contains(&self, reg: Reg) -> bool {
        let i = reg.0 as usize;
        self.0
            .get(i / 64)
            .is_some_and(|word| word & (1 << (i % 64)) != 0)
    }

    fn insert(&mut self, reg: Reg) {
        let i = reg.0 as usize;
        if self.0.len() <= i / 64 {
            self.0.resize(i / 64 + 1, 0);
        }
        self.0[i / 64] |= 1 << (i % 64);
    }

    fn remove(&mut self, reg: Reg) {
        let i = reg.0 as usize;
        if let Some(word) = self.0.get_mut(i / 64) {
            *word &= !(1 << (i % 64));
        }
    }

    /// Add every register of `from` that `kill` lacks, and answer whether the
    /// set grew.
    fn add_unkilled(&mut self, from: &RegSet, kill: &RegSet) -> bool {
        if self.0.len() < from.0.len() {
            self.0.resize(from.0.len(), 0);
        }
        let mut grew = false;
        for (i, &word) in from.0.iter().enumerate() {
            let killed = kill.0.get(i).copied().unwrap_or(0);
            let added = word & !killed & !self.0[i];
            self.0[i] |= added;
            grew |= added != 0;
        }
        grew
    }
}

/// The blocks control can reach straight from a block ending in `term`.
fn successors(term: &Terminator) -> Vec<Label> {
    match *term {
        Terminator::Jump(label) => vec![label],
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => vec![then_label, else_label],
        Terminator::Emit { resume_label, .. } => vec![resume_label],
        Terminator::Return(_) | Terminator::Unreachable => Vec::new(),
    }
}

/// The registers live into each block of `func`, by label.
pub(super) fn live_in(func: &LirView<'_>) -> FxHashMap<Label, RegSet> {
    let blocks: Vec<_> = func.blocks().collect();
    let index: FxHashMap<Label, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, block)| (block.label(), i))
        .collect();
    let succ: Vec<Vec<usize>> = blocks
        .iter()
        .map(|block| {
            successors(&block.terminator())
                .iter()
                .filter_map(|label| index.get(label).copied())
                .collect()
        })
        .collect();

    // Each block starts from what it reads before it writes, and records what
    // it writes, which hides a successor's read of the same register. The walk
    // runs backward, so a read that follows a write in the block counts for
    // nothing.
    let mut live = Vec::with_capacity(blocks.len());
    let mut kill = Vec::with_capacity(blocks.len());
    for block in &blocks {
        let mut reads = RegSet::default();
        let mut writes = RegSet::default();
        for_each_terminator_use(&block.terminator(), |reg| reads.insert(reg));
        for node in block.nodes().rev() {
            if let Some(reg) = node.def() {
                reads.remove(reg);
                writes.insert(reg);
            }
            for &reg in node.uses() {
                reads.insert(reg);
            }
        }
        live.push(reads);
        kill.push(writes);
    }

    // Then what its successors read, less what it writes, to a fixed point.
    let mut changed = true;
    while changed {
        changed = false;
        for b in (0..blocks.len()).rev() {
            for &s in &succ[b] {
                // A block adds nothing to itself: what it reads is already
                // in its set.
                let (to, from) = match b.cmp(&s) {
                    Ordering::Equal => continue,
                    Ordering::Less => {
                        let (head, tail) = live.split_at_mut(s);
                        (&mut head[b], &tail[0])
                    }
                    Ordering::Greater => {
                        let (head, tail) = live.split_at_mut(b);
                        (&mut tail[0], &head[s])
                    }
                };
                changed |= to.add_unkilled(from, &kill[b]);
            }
        }
    }

    blocks.iter().map(|block| block.label()).zip(live).collect()
}
