// audited: 2026-11-11
//! Pointer → region classification: the resolution behind every runtime RC decision.
//!
//! docs/impl/region/generations.md
//!
//! `region_of_ptr` answers `arena::region_of`, with the debug-build stale
//! check. `owning_region` answers the allocation funnel, the free cascade and
//! the mint-scope reclaim, which must not generation-check. Both read the
//! interval table before the page-base walk, so a pointer into a hydrated
//! image never reads a page header (docs/impl/image/hydration.md).

use super::super::regionpool::{header_of_page_ptr, region_of_page_ptr, PageStamp};
use super::*;
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static IMAGE_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The interval table: each hydrated region with the interval its pages tile,
/// in hydration order, and the hull that covers them all
/// (docs/impl/image/hydration.md).
#[derive(Default)]
pub(super) struct IntervalTable {
    entries: Vec<(RuntimeRegion, Range<usize>)>,
    /// The least interval that covers every entry; empty when there is none.
    hull: Range<usize>,
}

impl IntervalTable {
    /// Enter a hydrated region with the interval its pages tile.
    pub(super) fn push(&mut self, region: RuntimeRegion, interval: Range<usize>) {
        self.hull = if self.entries.is_empty() {
            interval.clone()
        } else {
            self.hull.start.min(interval.start)..self.hull.end.max(interval.end)
        };
        self.entries.push((region, interval));
    }

    /// Drop region `id`'s entry, if it has one, and recompute the hull from
    /// the entries left.
    pub(super) fn remove(&mut self, id: u32) {
        let before = self.entries.len();
        self.entries.retain(|(r, _)| r.get() != id);
        if self.entries.len() == before {
            return;
        }
        self.hull = self
            .entries
            .iter()
            .map(|(_, i)| i.clone())
            .reduce(|h, i| h.start.min(i.start)..h.end.max(i.end))
            .unwrap_or_default();
    }

    /// The region whose interval holds `addr`. An address outside the hull
    /// scans no entry.
    #[inline]
    fn find(&self, addr: usize) -> Option<u32> {
        if !self.hull.contains(&addr) {
            return None;
        }
        self.entries.iter().find_map(|(r, i)| {
            #[cfg(test)]
            IMAGE_PROBES.with(|n| n.set(n.get() + 1));
            i.contains(&addr).then_some(r.get())
        })
    }

    /// The table's regions, in hydration order.
    #[cfg(test)]
    pub(super) fn regions(&self) -> Vec<RuntimeRegion> {
        self.entries.iter().map(|(r, _)| *r).collect()
    }
}

impl RegionStore {
    /// The interval-table entries this thread has scanned, so a test can show
    /// that a lookup scanned none.
    #[cfg(test)]
    pub(crate) fn image_probes() -> u64 {
        IMAGE_PROBES.with(std::cell::Cell::get)
    }

    /// The least interval that covers every entry of the interval table, or
    /// `None` when the table is empty (docs/impl/image/hydration.md).
    #[cfg(test)]
    pub(crate) fn interval_hull(&self) -> Option<Range<usize>> {
        let hull = self.images.hull.clone();
        (!hull.is_empty()).then_some(hull)
    }

    /// Check if a pointer is owned by any region in this store.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.regions
            .iter()
            .any(|r| r.as_ref().is_some_and(|e| e.pool.owns(ptr)))
    }

    /// Region id of the page `ptr` points into (0 = no region page): the funnel
    /// through which every runtime RC decision classifies a value's region
    /// (docs/impl/region/generations.md).
    ///
    /// A hydrated region of this store whose interval holds `ptr` answers
    /// first. Otherwise the page-base walk stops at the first header that
    /// validates, and a live region of this store that owns `ptr` is the
    /// authoritative answer. Any other validated id is the fallback answer.
    /// Either this store stamped the page and the region is gone, a **stale
    /// deref** the debug generation check names here, or another store stamped
    /// it: a worker reading a parent-heap value, the tolerated cross-store
    /// borrow.
    pub fn region_of_ptr(&self, ptr: *const ()) -> u32 {
        if let Some(rid) = self.image_region_of(ptr as usize) {
            return rid;
        }
        // SAFETY: `ptr` is a heap value's payload, so it points into a region
        // page of this store or another, each with a header at its base.
        let (rid, stamp) = unsafe { header_of_page_ptr(ptr, self.pool.initial_page_size()) };
        if rid >= 2 && self.region_owns(rid, ptr) {
            return rid;
        }
        if cfg!(debug_assertions) && rid >= 2 && stamp.store == self.store_id {
            self.panic_if_stale(ptr, rid, stamp);
        }
        rid
    }

    /// The live region of this store that owns `ptr`, or 0 when none does.
    ///
    /// The resolver behind the allocation funnel's edge record, the free
    /// cascade's scan and fiber discharge, and the mint-scope reclaim. Like
    /// `region_of_ptr`, it reads the interval table before the page-base walk.
    /// It reads no generation, because teardown legitimately meets a value whose region
    /// is already gone, and it answers that value with 0. Ownership, not
    /// liveness alone, decides: a foreign pointer whose page header names a
    /// live local id resolves to 0, whenever it is asked.
    pub(crate) fn owning_region(&self, ptr: *const ()) -> u32 {
        if let Some(rid) = self.image_region_of(ptr as usize) {
            return rid;
        }
        // SAFETY: as in `region_of_ptr`.
        let rid = unsafe { region_of_page_ptr(ptr, self.pool.initial_page_size()) };
        if rid >= 2 && self.region_owns(rid, ptr) {
            rid
        } else {
            0
        }
    }

    /// The live hydrated region whose image interval holds `addr`, read from
    /// the interval table with no header read. A region its free has already
    /// unindexed answers nothing here.
    #[inline]
    fn image_region_of(&self, addr: usize) -> Option<u32> {
        self.images.find(addr)
    }

    /// Panic when this store stamped `ptr`'s page for region `rid` at a
    /// generation that is no longer current: the region was freed, and possibly
    /// recycled, after the value was made. The deref is the use-after-free site.
    fn panic_if_stale(&self, ptr: *const (), rid: u32, stamp: PageStamp) {
        let current = self.generation_raw(rid);
        if stamp.generation == current {
            return;
        }
        // With --trace=free or freebt, the free log lists every free that
        // reclaimed this page, oldest first, and the first is the original
        // over-free with its call site. Without the flag it is empty, and the
        // deref site alone still names the region.
        let attribution = {
            #[cfg(debug_assertions)]
            {
                crate::value::fiberheap::freelog::describe(ptr as usize)
                    .map(|s| format!("\n  {s}"))
                    .unwrap_or_default()
            }
            #[cfg(not(debug_assertions))]
            {
                String::new()
            }
        };
        panic!(
            "stale region deref: {ptr:?} points into a page stamped \
             region {rid} generation {}, but generation {current} is \
             current — region {rid} was freed (and possibly recycled) \
             after this Value was created; this deref is the \
             use-after-free site \
             (docs/impl/region/generations.md){attribution}",
            stamp.generation,
        );
    }

    /// Whether this store's region `rid` is live and `ptr` falls inside one of
    /// its pages. A mid-page false header names a region that does not own
    /// `ptr`, so this predicate rejects it.
    fn region_owns(&self, rid: u32, ptr: *const ()) -> bool {
        self.regions
            .get(rid as usize)
            .and_then(|slot| slot.as_ref())
            .is_some_and(|e| e.pool.owns(ptr))
    }
}
