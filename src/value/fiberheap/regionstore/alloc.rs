//! Minting, lazy region creation, and allocation into a region.
//!
//! Every allocation execution claims its own physical region id
//! (`new_runtime_region`); the entry behind that id is materialized lazily
//! on first touch (`ensure`/`ensure_raw`). Allocation funnels through here so
//! the cross-region incref that records outgoing edges happens exactly once
//! per stored object.

use super::*;

impl RegionStore {
    /// Mint a fresh physical region id. Each allocation *execution* claims
    /// its own region (docs/regions/semantics.md — every value its own region); the
    /// per-activation remap (in the VM) maps a static bytecode region slot
    /// to one of these per activation. Reuses a recycled id when available.
    pub fn new_runtime_region(&mut self) -> RuntimeRegion {
        // A minted id must not already name a *live* region. It can, because
        // physical ids enter this store from two independent sources: this
        // per-heap counter, and raw `new_static_region()` static-slot ids that
        // some paths use directly (or that `incref`/`ensure` re-animate after a
        // premature free). The two id ranges overlap, so `next_physical` (or a
        // recycled id) can land on a still-live region. Issuing it would alias
        // two logical regions onto one id → use-after-free when either is freed
        // (e.g. demos/fib/fib.lisp's torn-read abort). Skip any live id; the
        // skipped id stays owned by its current region until that region is
        // freed and recycles it. Terminates: `free_physical` is finite and a
        // brand-new `next_physical` id is never live.
        loop {
            let id = self.free_physical.pop().unwrap_or_else(|| {
                let id = self.next_physical;
                self.next_physical += 1;
                id
            });
            if (id as usize) >= self.regions.len() || self.regions[id as usize].is_none() {
                // Record the mint into the active closed-scope log (macro
                // expansion), stamping the generation `ensure_raw` will give
                // this id's pages so the reclaim pass can tell a still-live
                // region from a recycled-and-refreed one.
                if let Some(log) = self.mint_log.as_mut() {
                    let gen = self.generations.get(id as usize).copied().unwrap_or(0);
                    log.push((id, gen));
                }
                // `id` came from `next_physical` (≥ 2) or `free_physical` (a
                // recycled ≥ 2 id), so it is always nonzero.
                return RuntimeRegion::new(id).expect("minted physical id is nonzero");
            }
        }
    }

    /// Mint a physical region id together with the receipt that can return it —
    /// the mint used where the caller may end without allocating into the id
    /// (docs/impl/region/model.md § "Physical id recycling"). The per-call result
    /// region is that case: it is minted before the callee runs, because the
    /// callee may allocate its result into it, and a callee that returns an
    /// immediate or a borrowed value allocates nothing.
    pub fn new_runtime_region_tracked(&mut self) -> RegionMint {
        let region = self.new_runtime_region();
        RegionMint {
            region,
            gen: self.generation_raw(region.get()),
        }
    }

    /// Return a minted id to the free list when the mint never materialized it —
    /// the reserved → free exit of the id lifecycle (docs/impl/region/model.md
    /// § "Physical id recycling"). Without it an id that no allocation touched is
    /// stranded forever, and since `regions` is indexed by id, each stranded id
    /// is a table slot of resident memory that no heap gauge accounts for.
    ///
    /// Unmaterialized means two conditions together. `regions[id]` empty alone
    /// is also true of a region that materialized and was *freed* since the
    /// mint — whose teardown already pushed that id, so pushing it again would
    /// duplicate it in `free_physical`. A teardown bumps the id's generation, so
    /// requiring the generation to still equal the mint's admits only an id
    /// nothing has touched. Neither condition is redundant; `tests::recycle`
    /// pins what each one catches.
    pub fn recycle_unmaterialized(&mut self, mint: RegionMint) {
        let idx = mint.region().get() as usize;
        if idx < self.regions.len() && self.regions[idx].is_some() {
            return;
        }
        if self.generation_raw(mint.region().get()) != mint.gen {
            return;
        }
        self.free_physical.push(mint.region().get());
    }

    /// Ensure a region entry exists for `id`, creating it lazily.
    pub(super) fn ensure(&mut self, id: RuntimeRegion) {
        self.ensure_raw(id.get());
    }

    /// Ensure the entry for a raw physical id, creating it lazily. Mortal
    /// regions reach this through `ensure`; the raw form exists for the
    /// trace/alloc path that already holds a `u32`.
    pub(super) fn ensure_raw(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.regions.len() {
            // Backstop (always on, release included — the generation check in
            // `region_of_ptr` is debug-only): an id this large names no region a
            // real program can hold (see `MAX_PLAUSIBLE_REGION_ID`), so detonate
            // here, naming it, instead of resizing the table to hundreds of GB
            // and OOM-aborting far from the deref
            // (docs/impl/region/generations.md § "Region generations").
            //
            // The message names both admissible causes rather than asserting
            // one. The expected cause is a corrupt page-header read; the other
            // is a heap whose ids stopped recycling, which reaches this check
            // with ids that are sequential and a table that grew honestly. The
            // two are indistinguishable at this point, and naming only the first
            // sends a reader hunting corruption that is not there.
            assert!(
                id <= MAX_PLAUSIBLE_REGION_ID,
                "region id {id} (0x{id:08x}) reaching ensure_raw would grow the \
                 region table to {} entries — physically implausible. Either a \
                 corrupt page-header read (a misidentified page base, or a \
                 stale/foreign read) was handed back as a region id, or this \
                 heap's physical ids stopped returning to the free list \
                 (docs/impl/region/model.md § 'Physical id recycling'). Sequential \
                 ids and a live-region count far below this id point at the \
                 second. For the first, the in-situ detectors are the \
                 ownership-validated walk and generation check in region_of_ptr \
                 and --trace=guardfree (docs/impl/region/generations.md)",
                idx + 1,
            );
            self.regions.resize_with(idx + 1, || None);
        }
        if idx >= self.generations.len() {
            self.generations.resize(idx + 1, 0);
        }
        if self.regions[idx].is_none() {
            let stamp = super::super::regionpool::PageStamp {
                generation: self.generations[idx],
                store: self.store_id,
            };
            self.regions[idx] = Some(RegionEntry {
                pool: RegionPool::new(
                    id,
                    stamp,
                    self.pool.initial_page_size(),
                    std::sync::Arc::clone(&self.trace),
                ),
                reclaim: Reclaim::Counted(1),
                owned_children: Vec::new(),
                outgoing: FxHashMap::default(),
                incoming: FxHashMap::default(),
            });
        }
    }

    /// Allocate a HeapObject into a specific mortal region.
    /// Automatically increfs any cross-region Value refs in the object.
    pub fn alloc_obj(&mut self, id: RuntimeRegion, obj: HeapObject) -> Value {
        self.alloc_obj_raw(id.get(), obj)
    }

    fn alloc_obj_raw(&mut self, id: u32, obj: HeapObject) -> Value {
        self.ensure_raw(id);
        if crate::config::get().has_trace("rc") {
            let page_size = self.pool.initial_page_size();
            let valid_region = |rid: u32, ptr: *const ()| -> bool {
                self.regions
                    .get(rid as usize)
                    .and_then(|s| s.as_ref())
                    .is_some_and(|e| e.pool.owns(ptr))
            };
            let mut refs = Vec::new();
            RegionPool::find_object_cross_refs(&obj, id, page_size, &valid_region, &mut refs);
            if !refs.is_empty() {
                eprintln!(
                    "[trace:rc] alloc_obj({id}) xrefs={refs:?} tag={:?}",
                    obj.tag()
                );
            }
        }
        self.incref_cross_region_refs(&obj, id);
        let entry = self.regions[id as usize].as_mut().unwrap();
        entry.pool.alloc_obj(obj, &mut self.pool)
    }

    /// Allocate a `RegionSlice` into a specific mortal region.
    pub fn alloc_region_slice<T: Copy + 'static>(
        &mut self,
        id: RuntimeRegion,
        items: &[T],
    ) -> RegionSlice<T> {
        self.alloc_region_slice_raw(id.get(), items)
    }

    fn alloc_region_slice_raw<T: Copy + 'static>(
        &mut self,
        id: u32,
        items: &[T],
    ) -> RegionSlice<T> {
        self.ensure_raw(id);
        let entry = self.regions[id as usize].as_mut().unwrap();
        entry.pool.alloc_region_slice(items, &mut self.pool)
    }
}
