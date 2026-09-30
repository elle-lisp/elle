// audited: 2026-09-29
//! How a heap's regions end: the reclamation counters the `arena/*` gauges read.
//!
//! docs/impl/region/diagnostics.md

use super::*;

/// Every reclamation counter of one store, read at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReclaimCounters {
    pub region_frees: u64,
    pub page_frees: u64,
    pub object_frees: u64,
    pub adopts: u64,
    pub adopts_into_empty: u64,
    pub owned_frees: u64,
    pub owned_free_pages: u64,
    pub owned_free_objects: u64,
    pub owned_one_page_frees: u64,
    pub rescues: u64,
    pub rescue_survivors: u64,
    pub extracts: u64,
    pub reparents: u64,
}

impl RegionStore {
    /// The store's reclamation counters.
    #[cfg_attr(not(test), expect(dead_code, reason = "no gauge reads it yet"))]
    pub(crate) fn reclaim_counters(&self) -> ReclaimCounters {
        ReclaimCounters::default()
    }

    /// The regions owned now.
    #[cfg_attr(not(test), expect(dead_code, reason = "no gauge reads it yet"))]
    pub(crate) fn owned_count(&self) -> u64 {
        0
    }
}
