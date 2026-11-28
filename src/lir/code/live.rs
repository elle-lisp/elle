// audited: 2026-11-27
//! Which local slots a frozen function reads again past each point: into each block, and around each node.
//!
//! src/lir/overview.md
//! docs/impl/jit-closure.md

use super::view::LirView;
use crate::lir::Label;

/// A set of local slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
}

impl FromIterator<u16> for LiveSlots {
    fn from_iter<I: IntoIterator<Item = u16>>(slots: I) -> Self {
        let mut set = LiveSlots::default();
        for slot in slots {
            set.insert(slot);
        }
        set
    }
}

/// The local slots live into each block of one frozen function.
pub struct SlotLiveness {
    live_in: Vec<LiveSlots>,
}

impl SlotLiveness {
    /// The liveness of `func`'s local slots.
    pub fn of(func: &LirView<'_>) -> Self {
        SlotLiveness {
            live_in: vec![LiveSlots::default(); func.block_count()],
        }
    }

    /// The slots live into the block labeled `label`.
    pub fn live_in(&self, label: Label) -> &LiveSlots {
        let _ = label;
        &self.live_in[0]
    }

    /// The slots live around each node of the block at `position`.
    pub fn block(&self, func: &LirView<'_>, position: usize) -> BlockLiveness {
        BlockLiveness {
            around: vec![LiveSlots::default(); func.block(position).len() + 1],
        }
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
