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

mod alloc;
mod free;
mod introspect;
mod mintscope;
mod ownership;
mod pointer;
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
