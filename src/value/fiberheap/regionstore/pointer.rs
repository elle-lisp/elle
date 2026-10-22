// audited: 2026-10-21
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

impl RegionStore {
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
    /// the interval table with a range check per entry and no header read.
    /// A region its free has already unindexed answers nothing here.
    #[inline]
    fn image_region_of(&self, addr: usize) -> Option<u32> {
        self.images.iter().map(|r| r.get()).find(|&id| {
            self.regions
                .get(id as usize)
                .and_then(|slot| slot.as_ref())
                .is_some_and(|e| e.pool.image_contains(addr))
        })
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
