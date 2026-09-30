// audited: 2026-09-29
//! How a heap's regions end: the reclamation counters the `arena/*` gauges read.
//!
//! docs/impl/region/diagnostics.md

use super::*;

/// Every reclamation counter of one store, read at once. Each starts at 0 when
/// the store is made and never goes down, and a teardown moves none of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReclaimCounters {
    /// Regions freed, by a count reaching zero, an owner's drop, or a group free.
    pub region_frees: u64,
    /// The pages those regions held.
    pub page_frees: u64,
    /// The objects those regions held.
    pub object_frees: u64,
    /// Counted regions made members of an owner's subtree.
    pub adopts: u64,
    /// Adoptions into an owner that held no object at the adopt.
    pub adopts_into_empty: u64,
    /// Owned regions freed by their owner's drop.
    pub owned_frees: u64,
    /// The pages those owned regions held.
    pub owned_free_pages: u64,
    /// The objects those owned regions held.
    pub owned_free_objects: u64,
    /// The owned regions freed while they held one page or none.
    pub owned_one_page_frees: u64,
    /// Owned regions the drop-time rescue returned to counted.
    pub rescues: u64,
    /// Regions that outlived their owner's drop through a rescue: each rescued
    /// region and the owned subtree it keeps.
    pub rescue_survivors: u64,
    /// Owned regions a moves-out removal returned to counted.
    pub extracts: u64,
    /// Owned regions handed from one owner to another.
    pub reparents: u64,
}

impl ReclaimCounters {
    /// Count one region the free path tears down, `pages` and `objects` being
    /// what it held and `owned` whether its owner's drop is what freed it.
    pub(super) fn count_free(&mut self, pages: u64, objects: u64, owned: bool) {
        self.region_frees += 1;
        self.page_frees += pages;
        self.object_frees += objects;
        if owned {
            self.owned_frees += 1;
            self.owned_free_pages += pages;
            self.owned_free_objects += objects;
            if pages <= 1 {
                self.owned_one_page_frees += 1;
            }
        }
    }
}

impl RegionStore {
    /// The store's reclamation counters.
    pub(crate) fn reclaim_counters(&self) -> ReclaimCounters {
        self.counters
    }

    /// The regions owned now: a reading, not a count. A scan of the table, so
    /// it answers from the forest itself rather than from the counters it is
    /// checked against.
    pub(crate) fn owned_count(&self) -> u64 {
        self.regions
            .iter()
            .flatten()
            .filter(|e| matches!(e.reclaim, Reclaim::Owned { .. }))
            .count() as u64
    }
}
