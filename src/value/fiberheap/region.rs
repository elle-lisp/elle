//! `FiberHeap` region-allocator surface.
//!
//! Everything that allocates into, reference-counts, adopts, or inspects the
//! per-heap `RegionStore` lives here, separate from the instance-rider state
//! and the custom-allocator/teardown paths. These are inherent methods, so
//! method-call syntax resolves them without any re-export.

use crate::hir::region::RuntimeRegion;
use crate::value::heap::HeapObject;
use crate::value::Value;

use super::regionstore::RegionMint;
use super::FiberHeap;

impl FiberHeap {
    /// Allocate a HeapObject directly into a specific region.
    pub fn alloc_in_region(&mut self, obj: HeapObject, region_id: RuntimeRegion) -> Value {
        if let Some(limit) = self.object_limit {
            if self.alloc_count >= limit {
                self.alloc_error = Some((self.alloc_count, limit));
                return Value::NIL;
            }
        }

        let trace_rc = crate::config::get().has_trace("rc");
        let tag_dbg = if trace_rc { Some(obj.tag()) } else { None };
        let v = self.region_store.alloc_obj(region_id, obj);
        if trace_rc {
            eprintln!(
                "[trace:rc] alloc_in_region({region_id}) tag={:?} payload=0x{:x} count={}",
                tag_dbg.unwrap(),
                v.payload,
                self.alloc_count
            );
        }
        self.alloc_count += 1;
        self.note_mint();
        v
    }

    /// Record one mint after `alloc_count` was bumped: advance the live high-water
    /// mark and the monotonic cumulative counter. The sole `alloc_count += 1` site
    /// is `alloc_in_region`, so both derived counts update in lockstep with it.
    #[inline(always)]
    fn note_mint(&mut self) {
        if self.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.alloc_count;
        }
        self.total_alloc_count += 1;
    }

    /// Allocate a `RegionSlice` directly into a specific region.
    pub fn alloc_region_slice_in_region<T: Copy + 'static>(
        &mut self,
        items: &[T],
        region_id: RuntimeRegion,
    ) -> crate::value::region_slice::RegionSlice<T> {
        self.region_store.alloc_region_slice(region_id, items)
    }

    /// Mint a fresh **runtime** region id — a real, pages-owning region in the
    /// per-heap `RegionStore`, minted per allocation execution (recycled on
    /// free). The runtime counterpart of a compile-time `new_static_region`
    /// slot; the two id-spaces are distinct (see docs/impl/region/model.md § id-spaces).
    pub fn new_runtime_region(&mut self) -> RuntimeRegion {
        self.region_store.new_runtime_region()
    }

    /// Mint a fresh runtime region id together with the receipt that returns it
    /// if nothing allocates into it — the mint for a caller that may end without
    /// materializing its region (docs/impl/region/model.md § "Physical id
    /// recycling"). Pair with [`Self::recycle_unmaterialized_region`].
    pub(crate) fn new_runtime_region_tracked(&mut self) -> RegionMint {
        self.region_store.new_runtime_region_tracked()
    }

    /// Return a minted id to the free list if the mint never materialized it.
    /// A no-op when the id names a live region, or when it lived and died since
    /// the mint (docs/impl/region/model.md § "Physical id recycling").
    pub(crate) fn recycle_unmaterialized_region(&mut self, mint: RegionMint) {
        self.region_store.recycle_unmaterialized(mint);
    }

    /// Increment the reference count for a region.
    pub fn incref_region(&mut self, id: RuntimeRegion) {
        self.region_store.incref(id);
    }

    /// Decrement the reference count for a region.
    /// Decrements alloc_count if the region is freed.
    pub fn decref_region(&mut self, id: RuntimeRegion) {
        let freed = self.region_store.decref(id);
        self.alloc_count -= freed;
    }

    /// Release one reference to a runtime region: decref, and reclaim its pages
    /// (`free_runtime_region_pages`) only when RC reaches 0. Decrements
    /// alloc_count by the number of objects reclaimed.
    pub fn decref_region_if_present(&mut self, region: RuntimeRegion) {
        super::freelog::set_reason("decref_region_if_present (transient)");
        let freed = self.region_store.decref_if_present(region);
        self.alloc_count -= freed;
    }

    /// Record an outgoing content edge `src → dst` — the mutable-store seam's and
    /// fiber-signal funnel's hook into the §"The outgoing edge table" recorded table
    /// (docs/impl/region/ownership.md). `src` is the container/fiber's region, `dst` the
    /// stored value's; an immediate value (no region, `None`) or absent source is a
    /// no-op, and the reserved/self filter lives in `RegionStore::record_outgoing`.
    pub fn record_outgoing_edge(&mut self, src: Option<RuntimeRegion>, dst: Option<RuntimeRegion>) {
        if let (Some(s), Some(d)) = (src, dst) {
            self.region_store.record_outgoing(s.get(), d.get());
        }
    }

    /// Remove an outgoing content edge `src → dst` — the removal/overwrite half of
    /// the mutable-store seam (a pop/remove/del, or the old target of a replace).
    pub fn unrecord_outgoing_edge(
        &mut self,
        src: Option<RuntimeRegion>,
        dst: Option<RuntimeRegion>,
    ) {
        if let (Some(s), Some(d)) = (src, dst) {
            self.region_store.unrecord_outgoing(s.get(), d.get());
        }
    }

    /// Whether a region is currently an Owned forest member — the
    /// `AdoptIntoActivation` handlers' idempotence check (a re-delivered region
    /// keeps its first owner; docs/impl/region/owner.md § "Owner nodes").
    pub fn region_is_owned(&self, id: RuntimeRegion) -> bool {
        self.region_store.region_is_owned(id)
    }

    /// Link `child`'s region as an Owned member of `parent`'s region's subtree —
    /// the runtime `AdoptRegion` of the ownership forest (docs/impl/region/ownership.md
    /// § "Adoption and subtree drop"). Delegates to the region store, which freezes
    /// the child's RC so it is reclaimed only by `parent`'s subtree drop.
    pub fn adopt_region(&mut self, parent: RuntimeRegion, child: RuntimeRegion) {
        self.region_store.adopt_region(parent, child);
    }

    /// Hand every owned child of `from` to `to` — the ownership-transfer primitive
    /// of the forest (docs/impl/region/ownership.md § "The runtime: a reclamation
    /// typestate"). Move-only: each child is re-stamped to record `to` as its
    /// owner, so one set-drop at `to`'s demise reclaims them all.
    pub fn reparent_owned_children(&mut self, from: RuntimeRegion, to: RuntimeRegion) {
        self.region_store.reparent_owned_children(from, to);
    }

    /// Extract `child`'s region from its owner's subtree — the moves-out
    /// counterpart of `adopt_region` (docs/impl/region/ownership.md § "Adoption and
    /// subtree drop"). A `moves_out` funnel (`%pop`) removing an element that was
    /// adopted into its container's Owned subtree calls this so the element — now
    /// the call's escaping result — is no longer reclaimed by the container's
    /// subtree drop. Moves `child` from `Owned` to `Counted(1)` (the caller's one
    /// reference); a `Counted`/absent child is a no-op (the ordinary RC path).
    pub fn extract_owned_region(&mut self, child: RuntimeRegion) {
        self.region_store.extract_owned_region(child);
    }

    /// Free a co-owned region group as one unit — the runtime `FreeRegionGroup` of the
    /// ownership forest. A mutual reference cycle
    /// with no container parent has no owner among its members, so all are freed
    /// symmetrically at the group's collective last use; interior member↔member references
    /// reclaim with the group, genuinely-Shared frontier references cascade once.
    pub fn free_region_group(&mut self, members: &[RuntimeRegion]) -> usize {
        self.region_store.free_region_group(members)
    }

    /// Open a closed-scope mint log around a macro expansion
    /// (docs/impl/region/rules.md § "Macro expansion — a closed allocation
    /// scope"). Pair with `reclaim_macro_scope`.
    pub fn begin_region_mint_log(&mut self) {
        self.region_store.begin_mint_log();
    }

    /// Close the mint scope and reclaim its dead scratch by balancing each
    /// surviving region's unexplained references. `protected` regions (process
    /// roots) are never reclaimed. Decrements `alloc_count` by objects reclaimed.
    pub fn reclaim_region_mint_scope(&mut self, protected: &[RuntimeRegion]) {
        let freed = self.region_store.reclaim_mint_scope(protected);
        self.alloc_count -= freed;
    }

    /// Page size used by the region store's page pool.
    pub fn region_page_size(&self) -> usize {
        self.region_store.page_size()
    }

    /// Pages claimed from the region store's page pool since this heap was
    /// created — monotonic. The backend of the `arena/page-claims` gauge.
    pub fn page_claims(&self) -> u64 {
        self.region_store.page_claims()
    }

    /// Region id stamped on the page `ptr` points into (0 = no region page),
    /// with the debug-build stale-deref generation check (docs/impl/region/generations.md
    /// § "Region generations"). The backend of `arena::region_of`.
    pub fn region_of_ptr(&self, ptr: *const ()) -> u32 {
        self.region_store.region_of_ptr(ptr)
    }

    /// Get the reference count for a region.
    pub fn region_rc(&self, id: RuntimeRegion) -> u32 {
        self.region_store.rc(id)
    }

    /// The recorded outgoing edges of a region as `(target, count)` pairs — the
    /// test view of the §"The outgoing edge table" recorded table, for the
    /// mutable-store-seam tests that operate through a `FiberHeap`.
    #[cfg(test)]
    pub fn outgoing_edges(&self, id: RuntimeRegion) -> Vec<(u32, u32)> {
        self.region_store.outgoing_edges(id)
    }

    /// Current generation of a physical region id (docs/impl/region/generations.md
    /// § "Region generations"). The companion to `region_of_ptr` for the
    /// uncounted-borrow check: a reference that snapshots `(region, generation)`
    /// where it is established dangles once this value moves (the region's pages
    /// were freed since the snapshot). Unlike `region_of_ptr`, this reads no page
    /// header, so it is safe to call after the borrowed value's region was freed
    /// — it never derefs the (possibly stale) value.
    pub fn generation_raw(&self, id: u32) -> u32 {
        self.region_store.generation_raw(id)
    }

    /// Number of active regions.
    pub fn active_region_count(&self) -> usize {
        self.region_store.active_region_count()
    }

    /// Physical region ids this heap has issued — the backend of the
    /// `arena/region-ids` gauge (docs/impl/region/diagnostics.md). Flat in a
    /// steady-state loop; every unit of growth is an id that never returned to
    /// the free list.
    pub fn region_ids_issued(&self) -> u32 {
        self.region_store.region_ids_issued()
    }

    /// Entries in this heap's region table — what the table costs resident, in
    /// slots (docs/impl/region/model.md § "Physical id recycling").
    pub fn region_table_len(&self) -> usize {
        self.region_store.region_table_len()
    }

    /// Per-region info: (region_id, rc, object_count) for every active region.
    pub fn region_info_vec(&self) -> Vec<(u32, u32, usize)> {
        self.region_store.region_info_vec()
    }

    /// Dump every live mortal region (id, RC, object count, object tags) to
    /// stderr — the backend of the `arena/dump` leak-diagnostic primitive.
    pub fn debug_dump(&self) {
        self.region_store.debug_dump();
    }

    /// All cross-region reference edges among live mortal regions:
    /// `(referrer, referent)`, one entry per reference (not deduped), so a
    /// region's incoming-entry count is comparable to its rc — the residue
    /// leak-graph diagnostic.
    pub fn cross_ref_edges(&self) -> Vec<(u32, u32)> {
        self.region_store.cross_ref_edges()
    }

    /// Object tags currently live in a region (diagnostic).
    pub fn region_tags(&self, id: u32) -> Vec<crate::value::heap::HeapTag> {
        self.region_store.region_tags(id)
    }

    /// Check if a value is owned by any region in the RegionStore.
    pub fn value_in_region_store(&self, value: Value) -> bool {
        if !value.is_heap() {
            return false;
        }
        if let Some(ptr) = value.as_heap_ptr() {
            return self.region_store.owns(ptr);
        }
        false
    }
}
