// audited: 2026-11-27
//! Which local slots a frozen function reads again past each point: into each block, and around each node.
//!
//! src/lir/overview.md
//! docs/impl/jit-closure.md

use super::instr::InstrRef;
use super::slots::SlotRef;
use super::view::LirView;
use crate::lir::{Label, Terminator};
use std::collections::HashMap;

/// A set of local slots. Two sets that hold the same slots are equal, however
/// many words each holds.
#[derive(Clone, Debug, Default)]
pub struct LiveSlots(Vec<u64>);

impl LiveSlots {
    /// Whether `slot` is in the set.
    pub fn contains(&self, slot: u16) -> bool {
        let (word, bit) = (slot as usize / 64, slot % 64);
        self.0.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }

    /// The slots in the set, in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().enumerate().flat_map(|(i, &word)| {
            (0..64u16)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| i as u16 * 64 + bit)
        })
    }

    fn insert(&mut self, slot: u16) {
        let word = slot as usize / 64;
        if self.0.len() <= word {
            self.0.resize(word + 1, 0);
        }
        self.0[word] |= 1 << (slot % 64);
    }

    fn remove(&mut self, slot: u16) {
        if let Some(word) = self.0.get_mut(slot as usize / 64) {
            *word &= !(1 << (slot % 64));
        }
    }

    /// Add every slot of `other`, and answer whether the set grew.
    fn union_with(&mut self, other: &LiveSlots) -> bool {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        let mut grew = false;
        for (mine, &theirs) in self.0.iter_mut().zip(&other.0) {
            grew |= theirs & !*mine != 0;
            *mine |= theirs;
        }
        grew
    }

    /// The set as it stands before `instr`, from the set after it: a store
    /// ends the slot's life, and a read starts one.
    fn step_back(&mut self, instr: &InstrRef<'_>, func: &LirView<'_>) {
        instr.slot_refs(func, |slot| {
            if let SlotRef::Store(s) = slot {
                self.remove(s)
            }
        });
        instr.slot_refs(func, |slot| {
            if let SlotRef::Read(s) = slot {
                self.insert(s)
            }
        });
    }
}

impl PartialEq for LiveSlots {
    fn eq(&self, other: &Self) -> bool {
        let n = self.0.len().max(other.0.len());
        (0..n).all(|i| self.0.get(i).unwrap_or(&0) == other.0.get(i).unwrap_or(&0))
    }
}

impl Eq for LiveSlots {}

impl FromIterator<u16> for LiveSlots {
    fn from_iter<I: IntoIterator<Item = u16>>(slots: I) -> Self {
        let mut set = LiveSlots::default();
        for slot in slots {
            set.insert(slot);
        }
        set
    }
}

/// The labels control reaches when `term` ends its block. A `Return` and an
/// `Unreachable` reach none.
fn successors(term: &Terminator) -> impl Iterator<Item = Label> {
    let (first, second) = match *term {
        Terminator::Jump(label) => (Some(label), None),
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => (Some(then_label), Some(else_label)),
        Terminator::Emit { resume_label, .. } => (Some(resume_label), None),
        Terminator::Return(_) | Terminator::Unreachable => (None, None),
    };
    first.into_iter().chain(second)
}

/// The local slots live into each block of one frozen function.
pub struct SlotLiveness {
    /// Each block's position, by its label.
    positions: HashMap<Label, usize>,
    /// The slots live into each block, by position.
    live_in: Vec<LiveSlots>,
}

impl SlotLiveness {
    /// The liveness of `func`'s local slots: a backward walk over each block,
    /// repeated until no block's set grows. A loop's back edge carries its
    /// slots around on a later pass.
    pub fn of(func: &LirView<'_>) -> Self {
        let positions: HashMap<Label, usize> = func
            .blocks()
            .enumerate()
            .map(|(i, block)| (block.label(), i))
            .collect();
        // Each block as the slots it reads before any store replaces them, and
        // the slots it stores, so a pass costs a few words per block.
        let summaries: Vec<(LiveSlots, LiveSlots)> = func
            .blocks()
            .map(|block| {
                let (mut reads, mut stores) = (LiveSlots::default(), LiveSlots::default());
                for node in block.nodes().rev() {
                    let instr = node.instr();
                    instr.slot_refs(func, |slot| {
                        if let SlotRef::Store(s) = slot {
                            stores.insert(s)
                        }
                    });
                    reads.step_back(&instr, func);
                }
                (reads, stores)
            })
            .collect();
        let successors: Vec<Vec<usize>> = func
            .blocks()
            .map(|block| {
                successors(&block.terminator())
                    .filter_map(|label| positions.get(&label).copied())
                    .collect()
            })
            .collect();
        let mut live_in = vec![LiveSlots::default(); func.block_count()];
        let mut changed = true;
        while changed {
            changed = false;
            for i in (0..live_in.len()).rev() {
                let mut through = LiveSlots::default();
                for &s in &successors[i] {
                    through.union_with(&live_in[s]);
                }
                let (reads, stores) = &summaries[i];
                for slot in stores.iter() {
                    through.remove(slot);
                }
                through.union_with(reads);
                changed |= live_in[i].union_with(&through);
            }
        }
        SlotLiveness { positions, live_in }
    }

    /// The slots live into the block labeled `label`. Panics on a label no
    /// block carries.
    pub fn live_in(&self, label: Label) -> &LiveSlots {
        &self.live_in[self.positions[&label]]
    }

    /// The slots live around each node of the block at `position`.
    pub fn block(&self, func: &LirView<'_>, position: usize) -> BlockLiveness {
        let block = func.block(position);
        let mut live = LiveSlots::default();
        for label in successors(&block.terminator()) {
            live.union_with(self.live_in(label));
        }
        let mut around = vec![LiveSlots::default(); block.len() + 1];
        around[block.len()] = live.clone();
        for (i, node) in block.nodes().enumerate().rev() {
            live.step_back(&node.instr(), func);
            around[i] = live.clone();
        }
        BlockLiveness { around }
    }
}

/// The local slots live before and after each node of one block.
pub struct BlockLiveness {
    around: Vec<LiveSlots>,
}

impl BlockLiveness {
    /// The slots live as node `node` starts.
    pub fn before(&self, node: usize) -> &LiveSlots {
        &self.around[node]
    }

    /// The slots live once node `node` ends.
    pub fn after(&self, node: usize) -> &LiveSlots {
        &self.around[node + 1]
    }
}
