// audited: 2026-11-25
//! The dead regions' entries a store keeps, emptied, so the next region is built without allocating.
//!
//! docs/impl/region/ownership.md

use super::super::regionpool::PageStamp;
use super::*;

/// The most entries a store keeps.
pub(super) const KEPT_ENTRIES: usize = 64;

/// The most room a kept entry may hold in any one of its lists. A free drops an
/// entry with more, so a large region's lists do not stay resident.
pub(super) const KEPT_ENTRY_ROOM: usize = 64;

/// The least room a kept entry holds in each of its lists. A free keeps its
/// members' entries in one order and the next regions take them in another,
/// so an owner can take its child's entry. The floor makes any kept entry
/// serve any small region without allocating.
const KEPT_ENTRY_FLOOR: usize = 4;

/// What a store carries from one region to the next, so a region allocates
/// nothing once one of its shape has lived and died.
#[derive(Default)]
pub(super) struct Kept {
    /// Dead regions' entries, emptied, each with its lists' capacity.
    entries: Vec<RegionEntry>,
    /// The list the alloc funnel's cross-reference scan writes into.
    pub(super) xrefs: Vec<u32>,
}

impl RegionEntry {
    /// The most room this entry holds in any one of its lists.
    fn room(&self) -> usize {
        self.pool
            .room()
            .max(self.owned_children.capacity())
            .max(self.outgoing.capacity())
            .max(self.incoming.capacity())
    }
}

impl RegionStore {
    /// The entry `ensure_raw` makes `id` live with, stamped with the id's
    /// generation: a kept entry when the store has one, and a new one otherwise.
    pub(super) fn entry_for(&mut self, id: u32) -> RegionEntry {
        let stamp = PageStamp {
            generation: self.generations[id as usize],
            store: self.store_id,
        };
        let page = self.pool.initial_page_size();
        match self.kept.entries.pop() {
            Some(mut entry) => {
                entry.pool.reuse(id, stamp, page);
                entry.reclaim = Reclaim::Counted(1);
                entry
            }
            None => RegionEntry {
                pool: RegionPool::new(id, stamp, page, std::sync::Arc::clone(&self.trace)),
                reclaim: Reclaim::Counted(1),
                owned_children: Vec::new(),
                outgoing: FxHashMap::default(),
                incoming: FxHashMap::default(),
            },
        }
    }

    /// Keep the entry of a region whose pages a free has released, emptied,
    /// for the next region. Past either bound, drop it.
    pub(super) fn keep_entry(&mut self, mut entry: RegionEntry) {
        if self.kept.entries.len() >= KEPT_ENTRIES || entry.room() > KEPT_ENTRY_ROOM {
            return;
        }
        entry.owned_children.clear();
        entry.outgoing.clear();
        entry.incoming.clear();
        entry.owned_children.reserve(KEPT_ENTRY_FLOOR);
        entry.outgoing.reserve(KEPT_ENTRY_FLOOR);
        entry.incoming.reserve(KEPT_ENTRY_FLOOR);
        entry.pool.reserve(KEPT_ENTRY_FLOOR);
        self.kept.entries.push(entry);
    }

    /// How many entries the store keeps.
    #[cfg(test)]
    pub(crate) fn kept_entries(&self) -> usize {
        self.kept.entries.len()
    }
}
