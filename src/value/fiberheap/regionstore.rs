//! Region table: maps a physical region id → RegionEntry (RegionPool + RC).
//!
//! `RegionStore` lives on `FiberHeap` and owns the `PagePool` (per-thread
//! page cache). Each region is lazily created on first use.
//!
//! ## Reference counting
//!
//! Each region has a `u32` RC that starts at 1 (scope ref).
//! `incref(id)` / `decref(id)` adjust it. `decref_if_present(id)` is a
//! decref: if RC > 0 it decrements and frees when RC reaches 0.
//! The initial 1 is the scope's ref; cross-region refs add beyond that.
//!
//! ## Cascading frees
//!
//! When a region is freed, mutable collections in it may reference objects
//! in other regions. `teardown_and_cascade` walks collection contents and
//! decrefs each referenced region. This is a worklist, not recursion.

use super::pagepool::PagePool;
use super::regionpool::RegionPool;
use crate::hir::region::RuntimeRegion;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;
use rustc_hash::FxHashMap;

/// How a live region is reclaimed — a **typestate** (docs/impl/region/ownership.md
/// § "The runtime: a reclamation typestate"), not an `(rc, owner)` pair. A region
/// is reclaimed *exactly one way*, and the two ways are mutually exclusive
/// variants, so **owned-and-RC'd** — a region carrying both a live count and an
/// owner, where the count could independently free a region the owner will also
/// subtree-drop (a double-free) — is unrepresentable by construction. `adopt_region`
/// *moves* a region from `Counted` into `Owned`, consuming the count; once `Owned`
/// there is no `u32` left to decrement, so a stray decref is a structural no-op, not
/// a guard the decref path must remember to apply.
enum Reclaim {
    /// Shared / baseline: a cross-region reference count, starting at 1 (the scope
    /// ref). `decref` frees the region when it reaches 0.
    Counted(u32),
    /// Owned: reclaimed only by `owner`'s subtree drop (`free_runtime_region_pages`
    /// walking `owned_children`), never by its own count — it has none.
    Owned { owner: RuntimeRegion },
}

/// Per-region entry: storage pool + reclamation mode + ownership children.
struct RegionEntry {
    pool: RegionPool,
    /// Reclamation mode — counted (Shared/baseline) xor owned (forest member).
    /// See [`Reclaim`].
    reclaim: Reclaim,
    /// The regions this one owns (its direct children in the ownership forest).
    /// Freeing this region subtree-drops each of these recursively
    /// (`free_runtime_region_pages`). Empty for every region until an
    /// `AdoptRegion` links a child in. Independent of [`Reclaim`]: an interior
    /// node of a deep subtree is itself `Owned` *and* has `owned_children`.
    owned_children: Vec<RuntimeRegion>,
    /// Outgoing cross-region reference edges from this region: `target → count`
    /// (docs/impl/region/ownership.md § "The outgoing edge table"). The *content*
    /// edges — a `Value` in this region's heap objects pointing into another
    /// region — recorded at creation (the alloc funnel + the mutable-store seam +
    /// the fiber terminal-signal funnel) so reclamation walks this table
    /// (O(edges)) instead of scanning page contents. **Universal**: present on
    /// every region, `Owned` and `Counted` alike (an Owned region carries it for
    /// its cascade-on-drop but has no count). Mirrors exactly what
    /// `find_object_cross_refs` would find — same self/reserved-id filter — which
    /// the `#[cfg(debug_assertions)]` equivalence oracle in `free_region_set`
    /// asserts at every free. Distinct from the incoming RC (`Reclaim::Counted`),
    /// which also counts owner/transfer/borrow references the cascade never walks.
    outgoing: FxHashMap<RuntimeRegion, u32>,
}

impl RegionEntry {
    /// The region's independent reference count: the live count when `Counted`, and
    /// `0` when `Owned` — an owned region carries no count of its own (it is
    /// reclaimed by its owner's subtree drop). This is the single read path for RC,
    /// so every consumer sees an owned region as countless.
    fn count(&self) -> u32 {
        match self.reclaim {
            Reclaim::Counted(rc) => rc,
            Reclaim::Owned { .. } => 0,
        }
    }
}

/// Region table on FiberHeap.
pub(crate) struct RegionStore {
    /// Indexed by physical region id (mortal `RuntimeRegion`s, id ≥ 2).
    /// `None` = not yet created.
    regions: Vec<Option<RegionEntry>>,
    /// Per-thread page cache shared across all regions.
    pool: PagePool,
    /// Next never-used physical region id (monotonic). Id 0 is the
    /// unassigned/immediate sentinel and id 1 is reserved, so minting starts at 2.
    next_physical: u32,
    /// Recycled physical ids returned by `free_runtime_region_pages`. Reusing freed ids keeps
    /// the `regions` Vec bounded by the max *concurrently-live* region count
    /// even though allocation mints a fresh region per execution.
    free_physical: Vec<u32>,
    /// Per-physical-id generation counter (docs/impl/region/generations.md § "Region
    /// generations"), indexed like `regions`. Bumped on every path that
    /// returns an id's pages (RC-zero free, wholesale teardown); a recycled
    /// id mints its next region at the bumped generation. Each claimed page
    /// is stamped with its region's generation, and debug-build `region_of`
    /// compares stamp to counter — a mismatch is a stale deref, caught at
    /// the deref site instead of surfacing as a wrong read later.
    generations: Vec<u32>,
    /// Process-unique identity of this store, stamped into every page header
    /// it claims. Scopes the generation check: generations from two
    /// different stores are unrelated numbers, so a pointer into another
    /// store's page (worker thread reading a parent-heap value) is never
    /// generation-compared.
    store_id: u32,
    /// Active mint log for a *closed allocation scope* (macro expansion —
    /// docs/impl/region/rules.md § "Macro expansion — a closed allocation
    /// scope"). When `Some`, every id `new_runtime_region` mints is recorded
    /// with the generation it is about to be stamped at, so the scope's
    /// reclaim pass can balance each surviving region's unexplained references
    /// — and a recycled id (freed-and-reminted mid-scope) is distinguished
    /// from its earlier incarnation by generation. `None` outside such a scope
    /// (the common case: one branch on the mint path).
    mint_log: Option<Vec<(u32, u32)>>,
}

mod free;
mod mintscope;
mod refcount;

/// Mint for `RegionStore::store_id`: process-unique, starting at 1 so 0 (the
/// `PageStamp::default()` store) never names a real store.
static NEXT_STORE_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// Upper bound on a physically-plausible region id, the `ensure_raw` backstop's
/// tripwire (docs/impl/region/generations.md § "Region generations"). The region
/// table is indexed by id and bounded by the max *concurrently-live* regions —
/// freed ids recycle through `free_physical`, and static-slot ids are bounded by
/// the compiler's region-slot count — so a real id stays far below this. An id
/// above it reaching `ensure_raw` is a corrupt page-header read handed back as a
/// region id — a misidentified page base (the `region_of_ptr` walk's ownership
/// validation and the page-header magic close the known cause) or a stale/foreign
/// read — not a region to lazily create: resizing the table to it commits
/// hundreds of GB and OOM-aborts far from the deref. 2^28 ids is a ~36 GB table —
/// already impossible for a real program (each live region owns at least one
/// 4 KiB page) — while leaving headroom below the garbage range.
const MAX_PLAUSIBLE_REGION_ID: u32 = 1 << 28;

impl RegionStore {
    pub fn new(initial_page_size: usize, max_cached: usize) -> Self {
        RegionStore {
            regions: Vec::new(),
            pool: PagePool::new(initial_page_size, max_cached),
            next_physical: 2,
            free_physical: Vec::new(),
            generations: Vec::new(),
            store_id: NEXT_STORE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            mint_log: None,
        }
    }

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

    /// Ensure a region entry exists for `id`, creating it lazily.
    fn ensure(&mut self, id: RuntimeRegion) {
        self.ensure_raw(id.get());
    }

    /// Ensure the entry for a raw physical id, creating it lazily. Mortal
    /// regions reach this through [`ensure`]; the raw form exists for the
    /// trace/alloc path that already holds a `u32`.
    fn ensure_raw(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.regions.len() {
            // Backstop (always on, release included — the generation check in
            // `region_of_ptr` is debug-only): an id this large is never a real
            // region (see `MAX_PLAUSIBLE_REGION_ID`). It is a corrupt page-header
            // read handed back as a region id — a misidentified page base or a
            // stale/foreign read — so detonate here, naming it, instead of
            // resizing the table to ~584 GB and OOM-aborting far from the deref
            // (docs/impl/region/generations.md § "Region generations").
            assert!(
                id <= MAX_PLAUSIBLE_REGION_ID,
                "region id {id} (0x{id:08x}) reaching ensure_raw would grow the \
                 region table to {} entries — physically implausible; this is a \
                 corrupt page-header read (a misidentified page base, or a \
                 stale/foreign read) handed back as a region id. The in-situ \
                 detectors are the ownership-validated walk and generation check \
                 in region_of_ptr and --trace=guardfree \
                 (docs/impl/region/generations.md)",
                idx + 1,
            );
            self.regions.resize_with(idx + 1, || None);
        }
        if idx >= self.generations.len() {
            self.generations.resize(idx + 1, 0);
        }
        if self.regions[idx].is_none() {
            let stamp = super::regionpool::PageStamp {
                generation: self.generations[idx],
                store: self.store_id,
            };
            self.regions[idx] = Some(RegionEntry {
                pool: RegionPool::new(id, stamp, self.pool.initial_page_size()),
                reclaim: Reclaim::Counted(1),
                owned_children: Vec::new(),
                outgoing: FxHashMap::default(),
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

    /// Link `child` as an Owned member of `parent`'s subtree — the runtime
    /// `AdoptRegion` (docs/impl/region/ownership.md § "Adoption and subtree drop").
    /// **Moves** `child` from `Counted` into `Owned`, *consuming* its reference
    /// count: from here the child is reclaimed only by `parent`'s subtree drop
    /// (`free_runtime_region_pages`), never by its own RC reaching zero — there is
    /// no count left to reach zero. No incref — an interior ownership edge is not
    /// reference-counted (the subtree frees as a unit).
    ///
    /// A region is adopted **at most once**: a second adoption would mean two
    /// owners, so it finds the child already `Owned` and is a debug-asserted bug
    /// (the inference adopts each member once). This is the structural guard that
    /// "owned-and-RC'd" cannot arise — the count is gone after the first adoption,
    /// not merely frozen-and-ignored.
    ///
    /// Both regions are `ensure`d so the edge survives even if neither has
    /// allocated yet (a conditional alloc that never executed leaves an empty but
    /// present `Counted(1)` entry, exactly as the baseline RC path tolerates).
    pub(crate) fn adopt_region(&mut self, parent: RuntimeRegion, child: RuntimeRegion) {
        self.ensure(parent);
        self.ensure(child);
        let c = self.regions[child.get() as usize].as_mut().unwrap();
        debug_assert!(
            matches!(c.reclaim, Reclaim::Counted(_)),
            "region {child} adopted while already Owned — a region has at most one \
             owner; owned-and-RC'd is unrepresentable, so a double adoption is a bug \
             (docs/impl/region/ownership.md § 'The runtime: a reclamation typestate')",
        );
        c.reclaim = Reclaim::Owned { owner: parent };
        self.regions[parent.get() as usize]
            .as_mut()
            .unwrap()
            .owned_children
            .push(child);
    }

    /// Whether `id` is currently an **Owned** forest member (adopted — reclaimed
    /// only by its owner's subtree drop). False for an absent or `Counted`
    /// region. The `AdoptIntoActivation` handlers read this to make the
    /// consumer-facing adopt channel **idempotent**: a region delivered to the
    /// channel a second time (a masked-`:error` fiber restarted after handing
    /// out the same payload) is left with its first owner instead of tripping
    /// the one-owner assert in [`Self::adopt_region`].
    pub(crate) fn region_is_owned(&self, id: RuntimeRegion) -> bool {
        self.regions
            .get(id.get() as usize)
            .and_then(|s| s.as_ref())
            .is_some_and(|e| matches!(e.reclaim, Reclaim::Owned { .. }))
    }

    /// Hand `from`'s whole direct `owned_children` set to `to` — the ownership-
    /// **transfer** primitive of the forest (docs/impl/region/ownership.md § "The
    /// runtime: a reclamation typestate"). Each child is re-stamped
    /// `Owned { owner: to }` and the set is appended to `to`'s children: a move,
    /// never a copy, so the forest's forward/back edges stay consistent (the
    /// subtree-drop walk debug-asserts them) and no child gains a second owner.
    /// Neither endpoint's own reclaim mode changes and no count is created or
    /// consumed — the children were `Owned` and stay `Owned`; only the owner whose
    /// demise reclaims them changes. A self-reparent, an absent `from`, or an
    /// empty child set is a no-op (`to` is not even `ensure`d, so a transfer of
    /// nothing mints nothing).
    pub(crate) fn reparent_owned_children(&mut self, from: RuntimeRegion, to: RuntimeRegion) {
        if from == to {
            return;
        }
        let children = match self
            .regions
            .get_mut(from.get() as usize)
            .and_then(|s| s.as_mut())
        {
            Some(entry) => std::mem::take(&mut entry.owned_children),
            None => return,
        };
        if children.is_empty() {
            return;
        }
        self.ensure(to);
        for &child in &children {
            let entry = self.regions[child.get() as usize]
                .as_mut()
                .expect("an owned child has a live entry (freed only by its owner's drop)");
            debug_assert!(
                matches!(entry.reclaim, Reclaim::Owned { owner } if owner == from),
                "reparent_owned_children({from} -> {to}): child {child} does not \
                 record {from} as its owner — forward/back edge inconsistency \
                 (docs/impl/region/ownership.md § 'The runtime: a reclamation typestate')",
            );
            entry.reclaim = Reclaim::Owned { owner: to };
        }
        self.regions[to.get() as usize]
            .as_mut()
            .unwrap()
            .owned_children
            .extend(children);
    }

    /// Check if a pointer is owned by any region in this store.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.regions
            .iter()
            .any(|r| r.as_ref().is_some_and(|e| e.pool.owns(ptr)))
    }

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
    /// oracle's teeth can be pinned (`edges::oracle_panics_on_drift`). Never a
    /// production path.
    #[cfg(test)]
    pub fn force_outgoing_edge_for_test(&mut self, src: RuntimeRegion, dst: RuntimeRegion) {
        if let Some(e) = self
            .regions
            .get_mut(src.get() as usize)
            .and_then(|s| s.as_mut())
        {
            *e.outgoing.entry(dst).or_insert(0) += 1;
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

    /// Tear down all regions (fiber death). Bumps every torn-down id's
    /// generation, same as the RC-zero free path — a stale pointer into the
    /// returned pages must panic, not read.
    pub fn teardown_all(&mut self) {
        for idx in 0..self.regions.len() {
            if let Some(mut entry) = self.regions[idx].take() {
                entry.pool.teardown(&mut self.pool);
                self.bump_generation(idx as u32);
            }
        }
    }

    /// Bump the generation counter for a physical id whose pages were just
    /// returned. `ensure_raw` sized `generations` when the entry was
    /// created, so the slot always exists for a freed region.
    fn bump_generation(&mut self, id: u32) {
        let idx = id as usize;
        if idx < self.generations.len() {
            self.generations[idx] = self.generations[idx].wrapping_add(1);
        }
    }

    /// Current generation for a raw physical id (0 if never created).
    pub fn generation_raw(&self, id: u32) -> u32 {
        self.generations.get(id as usize).copied().unwrap_or(0)
    }

    /// Region id of the page `ptr` points into (0 = not a region page of this
    /// store) — the funnel through which every runtime RC decision classifies a
    /// value's region (docs/impl/region/generations.md § "Region generations").
    ///
    /// **Ownership-validated page-base walk.** A variable-sized page's base is
    /// found by masking `ptr` to each candidate power-of-2 alignment and reading
    /// the header there. The authoritative answer is the alignment whose header
    /// both self-validates ([`super::regionpool::header_if_valid`] — magic + size)
    /// AND names a live region of THIS store that genuinely owns `ptr`. A pointer
    /// deep inside a large page therefore resolves to its true base, never to a
    /// sub-aligned mid-page coincidence — that region would not own `ptr`. This
    /// is what closes the read of object data as a header that handed back a
    /// garbage id (`oracle.lisp`'s 584 GB `ensure_raw` blowup; pinned by
    /// `regionpool::tests` and the `regionstore::tests` walk).
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
            if let Some((rid, stamp)) = unsafe { super::regionpool::header_if_valid(addr, size) } {
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
                    assert!(
                        stamp.generation == current,
                        "stale region deref: {ptr:?} points into a page stamped \
                         region {rid} generation {}, but generation {current} is \
                         current — region {rid} was freed (and possibly recycled) \
                         after this Value was created; this deref is the \
                         use-after-free site \
                         (docs/impl/region/generations.md § 'Region generations')",
                        stamp.generation,
                    );
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

impl Drop for RegionStore {
    fn drop(&mut self) {
        self.teardown_all();
    }
}

impl Default for RegionStore {
    fn default() -> Self {
        Self::new(super::pagepool::BASE_PAGE, 4 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests;
