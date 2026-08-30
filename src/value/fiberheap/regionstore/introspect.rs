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

    /// Pages this store has claimed from its pool, fresh mappings and recycled
    /// pages alike — monotonic, never decremented on release. The backend of
    /// the `arena/page-claims` gauge (docs/impl/region/diagnostics.md): a delta
    /// across a fixed window is a shape's *page* cost, which the object and
    /// region gauges do not show.
    pub fn page_claims(&self) -> u64 {
        self.pool.counters().claims()
    }

    /// Number of active (non-empty) regions.
    pub fn active_region_count(&self) -> usize {
        self.regions.iter().filter(|r| r.is_some()).count()
    }

    /// Physical region ids issued — one past the largest id ever minted from
    /// scratch, so it is the high-water mark of ids in simultaneous circulation.
    /// The backend of the `arena/region-ids` gauge
    /// (docs/impl/region/diagnostics.md).
    ///
    /// The *id* dimension, which the object, byte, and page gauges cannot show: a
    /// minted id that never allocates holds none of what they count, yet keeps
    /// its id out of circulation forever (docs/impl/region/model.md § "Physical
    /// id recycling"). A mint that finds `free_physical` empty raises this;
    /// a mint that recycles leaves it alone. So a steady-state loop holds it
    /// flat, and every unit of growth is an id that did not come back.
    ///
    /// This leads [`Self::region_table_len`], which only moves when an id is made
    /// *live*: stranded ids are never materialized, so they inflate the table
    /// only once some later mint reaches their range. Read this one to detect an
    /// id leak, and the table length for what it costs in resident memory.
    pub fn region_ids_issued(&self) -> u32 {
        self.next_physical
    }

    /// Entries in the region table — one past the largest physical id ever made
    /// live, since `ensure_raw` sizes the table to the id it materializes. Its
    /// slot size times this is what the table costs resident.
    pub fn region_table_len(&self) -> usize {
        self.regions.len()
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

    /// Clone the `data` handle of every live `External` object whose Elle-side
    /// type name is `type_name`, across all regions. Type-agnostic here (the
    /// value layer never downcasts) — the caller downcasts and acts on each.
    ///
    /// A program that ends without letting go of an io-backend strands it to
    /// teardown, with its in-flight ops' `Port`/`ProcessHandle` values still
    /// filed. Quiescing each backend BEFORE the id-ordered free sweep — while
    /// every value is still there — keeps the drain from reading, and releasing,
    /// what an earlier region in the same sweep already freed (src/io/AGENTS.md
    /// § "A hold is let go while its store is still there").
    pub fn collect_external_data(&self, type_name: &str) -> Vec<std::rc::Rc<dyn std::any::Any>> {
        let mut out = Vec::new();
        for slot in self.regions.iter() {
            let Some(e) = slot.as_ref() else { continue };
            for obj in e.pool.live_objects() {
                if let crate::value::heap::HeapObject::External { obj: ext, .. } = obj {
                    if ext.type_name == type_name {
                        out.push(ext.data.clone());
                    }
                }
            }
        }
        out
    }
}
