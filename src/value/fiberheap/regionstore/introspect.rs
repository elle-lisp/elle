//! Read-only introspection: counts, byte totals, cross-ref/edge dumps, and the
//! generation accessor. These back the `arena/*` diagnostics and the free-time
//! equivalence oracle's comparison views; none mutate reclamation state. The
//! `#[cfg(test)]` helpers here expose the same internals to the region tests.

use super::*;

impl RegionStore {
    #[cfg(test)]
    pub fn region_obj_count(&self, id: RuntimeRegion) -> usize {
        let idx = id.get() as usize;
        if idx < self.regions.len() {
            self.regions[idx].as_ref().map_or(0, |e| e.pool.obj_count())
        } else {
            0
        }
    }

    /// The recorded outgoing edges of a region as `(target, count)` pairs,
    /// sorted by target id — the test view of `RegionEntry::outgoing` (the §
    /// "The outgoing edge table" recorded view, distinct from the `cross_ref_edges`
    /// content scan, so a test can pin the two equal). Empty for an absent region.
    #[cfg(test)]
    pub fn outgoing_edges(&self, id: RuntimeRegion) -> Vec<(u32, u32)> {
        let idx = id.get() as usize;
        let mut edges: Vec<(u32, u32)> = self
            .regions
            .get(idx)
            .and_then(|s| s.as_ref())
            .map(|e| e.outgoing.iter().map(|(t, &c)| (t.get(), c)).collect())
            .unwrap_or_default();
        edges.sort_unstable();
        edges
    }

    /// Inject a spurious outgoing edge `src → dst`, bypassing the record filter —
    /// a test-only way to manufacture table/scan drift so the free-time equivalence
    /// oracle's teeth can be pinned (`edges::oracle_panics_on_drift`). The incoming
    /// mirror IS maintained: the drift under test is recorded-table vs content,
    /// not the outgoing/incoming lockstep (whose own tripwire is
    /// `unmirror_incoming`'s debug assert). Never a production path.
    #[cfg(test)]
    pub fn force_outgoing_edge_for_test(&mut self, src: RuntimeRegion, dst: RuntimeRegion) {
        if let Some(e) = self
            .regions
            .get_mut(src.get() as usize)
            .and_then(|s| s.as_mut())
        {
            *e.outgoing.entry(dst).or_insert(0) += 1;
        } else {
            return;
        }
        if let Some(e) = self
            .regions
            .get_mut(dst.get() as usize)
            .and_then(|s| s.as_mut())
        {
            *e.incoming.entry(src).or_insert(0) += 1;
        }
    }

    /// Sum of live objects across every active region. Unlike the
    /// `FiberHeap::alloc_count` running counter (incremented at alloc,
    /// decremented only on the decref/decref_if_present paths), this reads the
    /// current per-region object counts directly, so it tracks ALL
    /// reclamation — including scope-region resets that recycle a region's
    /// pages without flowing through `decref_region`. That makes it the
    /// trustworthy basis for `arena/count`.
    pub fn total_obj_count(&self) -> usize {
        self.regions
            .iter()
            .filter_map(|r| r.as_ref())
            .map(|e| e.pool.obj_count())
            .sum()
    }

    /// Total allocated bytes across all regions + cached pages.
    pub fn allocated_bytes(&self) -> usize {
        let region_bytes: usize = self
            .regions
            .iter()
            .filter_map(|r| r.as_ref())
            .map(|e| e.pool.allocated_bytes())
            .sum();
        region_bytes + self.pool.cached_bytes()
    }

    /// Page size used by this store's pool.
    pub fn page_size(&self) -> usize {
        self.pool.initial_page_size()
    }

    /// Number of active (non-empty) regions.
    pub fn active_region_count(&self) -> usize {
        self.regions.iter().filter(|r| r.is_some()).count()
    }

    /// Object tags currently live in a region (the `arena/dump` diagnostic).
    pub fn region_tags(&self, id: u32) -> Vec<crate::value::heap::HeapTag> {
        self.regions
            .get(id as usize)
            .and_then(|s| s.as_ref())
            .map(|e| e.pool.debug_tags())
            .unwrap_or_default()
    }

    pub fn debug_dump(&self) {
        for (idx, slot) in self.regions.iter().enumerate() {
            if let Some(e) = slot.as_ref() {
                eprintln!(
                    "  region {} rc={} objs={} tags={:?}",
                    idx,
                    e.count(),
                    e.pool.obj_count(),
                    e.pool.debug_tags()
                );
            }
        }
    }

    /// All cross-region reference edges among live mortal regions:
    /// `(referrer, referent)`, one entry per reference (not deduped), so a
    /// region's incoming-entry count is comparable to its rc. Diagnostic
    /// counterpart of the free-time cascade scan — `rc - in_degree` is the
    /// number of references a region's RC carries that are NOT explained by
    /// live region contents (owner/escape references never released).
    pub fn cross_ref_edges(&self) -> Vec<(u32, u32)> {
        let page_size = self.pool.initial_page_size();
        let valid = |rid: u32, ptr: *const ()| {
            self.regions
                .get(rid as usize)
                .and_then(|s| s.as_ref())
                .is_some_and(|e| e.pool.owns(ptr))
        };
        let mut edges = Vec::new();
        for (idx, slot) in self.regions.iter().enumerate() {
            if let Some(e) = slot.as_ref() {
                for to in e.pool.find_region_cross_refs(idx as u32, page_size, &valid) {
                    edges.push((idx as u32, to));
                }
            }
        }
        edges
    }

    /// Per-region info: (region_id, rc, object_count) for every active region.
    pub fn region_info_vec(&self) -> Vec<(u32, u32, usize)> {
        self.regions
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| {
                slot.as_ref()
                    .map(|e| (idx as u32, e.count(), e.pool.obj_count()))
            })
            .collect()
    }

    /// Current generation for a raw physical id (0 if never created).
    pub fn generation_raw(&self, id: u32) -> u32 {
        self.generations.get(id as usize).copied().unwrap_or(0)
    }
}
