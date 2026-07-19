//! Per-instance heap ownership.
//!
//! `FiberHeap` uses `RegionStore` — a physical region allocator where
//! each region owns its pages exclusively. `FreeRegion(ρ)` tears down
//! the region's pages (with cascade decref for cross-region refs).

use std::rc::Rc;

use crate::hir::region::RuntimeRegion;
use crate::value::allocator::AllocatorBox;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

pub(crate) mod freelog;
pub(crate) mod pagepool;
pub(crate) mod regionpool;
pub(crate) mod regionstore;

// The `FiberHeap` surface is split by concern across sibling submodules; each
// is an inherent `impl` block or `pub(crate)` free fn, so callers reach them by
// method-call or path resolution unchanged. See each module's own docs.
mod custom;
mod dropsafety;
mod region;

pub(crate) use dropsafety::{holds_value_refs, needs_drop};

use regionstore::RegionStore;

/// Print the page-claim size histogram to stderr under `--stats` (a page-size
/// analysis aid; see `pagepool::dump_page_hist`). Reached from the `--stats`
/// exit path and registered as an at-exit dump for the `os/exit` test-runner path.
pub fn dump_page_hist() {
    pagepool::dump_page_hist();
}

/// Tracks objects allocated by a single `with-allocator` invocation.
pub(crate) struct CustomAllocState {
    allocator: Rc<AllocatorBox>,
    /// Objects allocated by this custom allocator.
    /// Each entry is (ptr, size, align) matching the alloc() call.
    custom_ptrs: Vec<(*mut u8, usize, usize)>,
    /// Pointers to HeapObjects that need Drop, owned by this allocator.
    dtors: Vec<*mut HeapObject>,
}

pub struct FiberHeap {
    /// Physical region allocator: each region owns its pages exclusively.
    region_store: RegionStore,
    /// Total allocation count (across all regions + custom allocators).
    alloc_count: usize,
    /// Peak number of objects allocated (high-water mark).
    peak_alloc_count: usize,
    /// Stack of custom allocators. The top is active.
    custom_alloc_stack: Vec<CustomAllocState>,
    /// Maximum number of objects this fiber may allocate. `None` = unlimited.
    object_limit: Option<usize>,
    /// Allocation limit violation flag.
    alloc_error: Option<(usize, usize)>,
    /// This instance's default trait tables, indexed by `HeapTag as usize`. Built
    /// lazily into this heap's `root_region` by `traitregistry::init_default_traits`,
    /// read on the collection-ctor path via [`Self::default_traits_for`]. Empty
    /// until built; an unbuilt/out-of-range tag reads `NIL`. Two coexisting
    /// instances each carry their own, so a value built in instance A never points
    /// at instance B's trait struct.
    default_traits: Vec<Value>,
    /// This instance's pinned process-lifetime root region for its startup-once
    /// roots (the default trait tables), held alive by RC. Minted lazily on first
    /// `alloc_root`.
    root_region: Option<RuntimeRegion>,
    /// Regions held on behalf of this instance — resident roots the teardown
    /// sweep releases by RC (decref once) so their graph can be reclaimed.
    process_roots: Vec<RuntimeRegion>,
    /// This instance's authoritative trace bitfield (`--trace=` / runtime
    /// `(vm/config-set :trace …)`). The VM's `RuntimeConfig` and the region
    /// pool's `PAGES` gate each hold a clone of this one cell, so a diagnostic
    /// toggle is scoped to this instance — two coexisting heaps never share it.
    trace: crate::config::TraceCell,
}

impl FiberHeap {
    pub fn new() -> Self {
        let cfg = crate::config::get();
        let trace: crate::config::TraceCell =
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        FiberHeap {
            region_store: RegionStore::new(
                cfg.region_page_size,
                cfg.page_pool_max,
                std::sync::Arc::clone(&trace),
            ),
            alloc_count: 0,
            peak_alloc_count: 0,
            custom_alloc_stack: Vec::new(),
            object_limit: None,
            alloc_error: None,
            default_traits: Vec::new(),
            root_region: None,
            process_roots: Vec::new(),
            trace,
        }
    }

    /// A clone of this instance's trace cell, for a reader that lives off the VM
    /// (the VM's `RuntimeConfig`, a channel's `WakeList`, a spawned worker). Every
    /// clone reads and writes the same bitfield, so the whole instance shares one
    /// trace state.
    pub fn trace_cell(&self) -> crate::config::TraceCell {
        std::sync::Arc::clone(&self.trace)
    }

    // ── Instance-owned rider state ──────────────────────────────────────
    //
    // The default trait tables, the pinned root region, and the process-root
    // registry are per-instance facts that live on the heap, so two embedded
    // instances on one thread each carry their own.

    /// The default traitset for `tag` on THIS instance (`NIL` if unbuilt).
    #[inline]
    pub fn default_traits_for(&self, tag: HeapTag) -> Value {
        self.default_traits
            .get(tag as usize)
            .copied()
            .unwrap_or(Value::NIL)
    }

    /// Whether this instance's default trait tables have been built.
    pub fn default_traits_built(&self) -> bool {
        !self.default_traits.is_empty()
    }

    /// Install this instance's default trait table (indexed by `HeapTag as usize`).
    pub fn set_default_traits(&mut self, table: Vec<Value>) {
        self.default_traits = table;
    }

    /// This instance's pinned root region, if minted.
    pub fn root_region_slot(&self) -> Option<RuntimeRegion> {
        self.root_region
    }

    /// Set (or clear) this instance's pinned root region.
    pub fn set_root_region(&mut self, region: Option<RuntimeRegion>) {
        self.root_region = region;
    }

    /// Record `region` as a process root of this instance.
    pub fn register_process_root_region(&mut self, region: RuntimeRegion) {
        self.process_roots.push(region);
    }

    /// A snapshot of this instance's process roots (for the macro-scope
    /// protected-set; does not drain).
    pub fn process_roots_snapshot(&self) -> Vec<RuntimeRegion> {
        self.process_roots.clone()
    }

    /// Drain and return this instance's process roots (teardown releases each).
    pub fn take_process_roots(&mut self) -> Vec<RuntimeRegion> {
        std::mem::take(&mut self.process_roots)
    }

    /// Object count (used by arena primitives and object limiting).
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    /// Live object count for `arena/count`: the sum of every active
    /// region's current object count. This reflects real reclamation
    /// (scope-region resets, flip/rotation page recycling, and RC frees
    /// alike), unlike the `alloc_count` running counter which only moves
    /// on the decref/decref_if_present paths and so over-reports phantom
    /// "leaks" that RSS contradicts.
    pub fn visible_len(&self) -> usize {
        self.region_store.total_obj_count()
    }

    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    /// Get the current object limit.
    pub fn object_limit(&self) -> Option<usize> {
        self.object_limit
    }

    /// Set the object limit. Returns the previous limit.
    pub fn set_object_limit(&mut self, limit: Option<usize>) -> Option<usize> {
        let prev = self.object_limit;
        self.object_limit = limit;
        prev
    }

    /// Take the allocation error flag, clearing it.
    pub fn take_alloc_error(&mut self) -> Option<(usize, usize)> {
        self.alloc_error.take()
    }

    /// Bytes committed by the region store.
    pub fn allocated_bytes(&self) -> usize {
        self.region_store.allocated_bytes()
    }

    /// Clone the `data` handle of every live `External` object whose Elle-side
    /// type name is `type_name`. The full-module WASM tier uses this to quiesce
    /// stranded io-backend externals before the region free-sweep
    /// (`RegionStore::collect_external_data`, docs/impl/wasm.md § the posix gap).
    pub fn collect_external_data(&self, type_name: &str) -> Vec<std::rc::Rc<dyn std::any::Any>> {
        self.region_store.collect_external_data(type_name)
    }

    /// Peak number of objects allocated (high-water mark).
    pub fn peak_alloc_count(&self) -> usize {
        self.peak_alloc_count
    }

    /// Reset peak to current count. Returns previous peak.
    pub fn reset_peak(&mut self) -> usize {
        let prev = self.peak_alloc_count;
        self.peak_alloc_count = self.alloc_count;
        prev
    }

    #[inline(always)]
    #[allow(dead_code)]
    pub(crate) fn trace_rc() -> bool {
        crate::config::get().trace_bits() & crate::config::trace_bits::RC != 0
    }
}

impl Default for FiberHeap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
