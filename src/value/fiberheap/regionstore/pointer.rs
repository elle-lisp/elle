//! Pointer → region classification.
//!
//! Every runtime RC decision starts by asking which region a `Value` lives in.
//! `region_of_ptr` answers with an ownership-validated page-base walk so a
//! pointer deep inside a large page resolves to its true base, never to a
//! sub-aligned mid-page coincidence (docs/impl/region/generations.md §
//! "Region generations").

use super::*;

impl RegionStore {
    /// Check if a pointer is owned by any region in this store.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.regions
            .iter()
            .any(|r| r.as_ref().is_some_and(|e| e.pool.owns(ptr)))
    }

    /// Region id of the page `ptr` points into (0 = not a region page of this
    /// store) — the funnel through which every runtime RC decision classifies a
    /// value's region (docs/impl/region/generations.md § "Region generations").
    ///
    /// **Ownership-validated page-base walk.** A variable-sized page's base is
    /// found by masking `ptr` to each candidate power-of-2 alignment and reading
    /// the header there. The authoritative answer is the alignment whose header
    /// both self-validates ([`super::super::regionpool::header_if_valid`] —
    /// magic + size) AND names a live region of THIS store that genuinely owns
    /// `ptr`. A pointer deep inside a large page therefore resolves to its true
    /// base, never to a sub-aligned mid-page coincidence — that region would not
    /// own `ptr`. This is what closes the read of object data as a header that
    /// handed back a garbage id (`oracle.lisp`'s 584 GB `ensure_raw` blowup;
    /// pinned by `regionpool::tests` and the `regionstore::tests` walk).
    ///
    /// When no live owned region claims `ptr`, the first self-validating header
    /// is the fallback id:
    /// - one THIS store stamped whose region is gone — a **stale deref** (the
    ///   region was freed/recycled); the debug generation check names it here.
    /// - one from **another store** (a worker reading a parent-heap value),
    ///   reported with its id (the tolerated cross-store borrow).
    pub fn region_of_ptr(&self, ptr: *const ()) -> u32 {
        let addr = ptr as usize;
        let mut size = self.pool.initial_page_size();
        while size != 0 {
            if let Some((rid, stamp)) =
                unsafe { super::super::regionpool::header_if_valid(addr, size) }
            {
                // A self-validating header (magic + size): a REAL page base of
                // this size. The magic makes mid-page object data fail
                // validation, so a large page's smaller sub-alignments are
                // skipped and the walk reaches the true base here — closing the
                // read-data-as-a-header bug. Stop here: continuing past a real
                // base would mask to addresses *below* this page (unmapped). The
                // `ensure_raw` backstop catches the ~1/2^32 residual where
                // mid-page data carries the magic by chance.
                if rid >= 2 && self.region_owns(rid, ptr) {
                    // Authoritative: this store's live region `rid` owns `ptr`.
                    return rid;
                }
                if cfg!(debug_assertions) && rid >= 2 && stamp.store == self.store_id {
                    // This store stamped this base but no longer owns `ptr`: a
                    // stale deref — the region was freed (and possibly recycled).
                    // The generation check names it at the deref.
                    let current = self.generation_raw(rid);
                    if stamp.generation != current {
                        // Attribute the premature free: with --trace=free/freebt the
                        // free-log lists every free that reclaimed this page, oldest
                        // first (the first is the original over-free, with its call
                        // site). Empty without the flag — the deref site alone still
                        // names the region.
                        let attribution = {
                            #[cfg(debug_assertions)]
                            {
                                crate::value::fiberheap::freelog::describe(addr)
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
                             (docs/impl/region/generations.md § 'Region generations'){attribution}",
                            stamp.generation,
                        );
                    }
                }
                // Not owned by this store: a stale own-store page, a foreign page
                // (a worker reading a parent-heap value — the tolerated
                // cross-store borrow, reported with its id as before), or — with
                // vanishing probability — a magic coincidence the backstop
                // catches.
                return rid;
            }
            size <<= 1;
        }
        0
    }

    /// Whether this store's region `rid` is live and `ptr` falls inside one of
    /// its pages — the ownership predicate that makes [`Self::region_of_ptr`]'s
    /// walk authoritative: a mid-page false match names a region that does not
    /// own `ptr`, so it is rejected in favour of the true owning base.
    fn region_owns(&self, rid: u32, ptr: *const ()) -> bool {
        self.regions
            .get(rid as usize)
            .and_then(|slot| slot.as_ref())
            .is_some_and(|e| e.pool.owns(ptr))
    }
}
