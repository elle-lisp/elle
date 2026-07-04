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
}

impl FiberHeap {
    pub fn new() -> Self {
        let cfg = crate::config::get();
        FiberHeap {
            region_store: RegionStore::new(cfg.region_page_size, cfg.page_pool_max),
            alloc_count: 0,
            peak_alloc_count: 0,
            custom_alloc_stack: Vec::new(),
            object_limit: None,
            alloc_error: None,
            default_traits: Vec::new(),
            root_region: None,
            process_roots: Vec::new(),
        }
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

    // ── Region allocator ────────────────────────────────────────────

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
        if self.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.alloc_count;
        }
        v
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
    /// slot; the two id-spaces are distinct (see docs/impl/region-model.md § id-spaces).
    pub fn new_runtime_region(&mut self) -> RuntimeRegion {
        self.region_store.new_runtime_region()
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
        freelog::set_reason("decref_region_if_present (transient)");
        let freed = self.region_store.decref_if_present(region);
        self.alloc_count -= freed;
    }

    /// Record an outgoing content edge `src → dst` — the mutable-store seam's and
    /// fiber-signal funnel's hook into the §"The outgoing edge table" recorded table
    /// (docs/impl/region-model.md). `src` is the container/fiber's region, `dst` the
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
    /// keeps its first owner; docs/impl/region-model.md § "Owner nodes").
    pub fn region_is_owned(&self, id: RuntimeRegion) -> bool {
        self.region_store.region_is_owned(id)
    }

    /// Link `child`'s region as an Owned member of `parent`'s region's subtree —
    /// the runtime `AdoptRegion` of the ownership forest (docs/impl/region-model.md
    /// § "Adoption and subtree drop"). Delegates to the region store, which freezes
    /// the child's RC so it is reclaimed only by `parent`'s subtree drop.
    pub fn adopt_region(&mut self, parent: RuntimeRegion, child: RuntimeRegion) {
        self.region_store.adopt_region(parent, child);
    }

    /// Hand every owned child of `from` to `to` — the ownership-transfer primitive
    /// of the forest (docs/impl/region-model.md § "The runtime: a reclamation
    /// typestate"). Move-only: each child is re-stamped to record `to` as its
    /// owner, so one set-drop at `to`'s demise reclaims them all.
    pub fn reparent_owned_children(&mut self, from: RuntimeRegion, to: RuntimeRegion) {
        self.region_store.reparent_owned_children(from, to);
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
    /// (docs/impl/region-rules.md § "Macro expansion — a closed allocation
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

    /// Region id stamped on the page `ptr` points into (0 = no region page),
    /// with the debug-build stale-deref generation check (docs/impl/region-generations.md
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

    /// Current generation of a physical region id (docs/impl/region-generations.md
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

    // ── Custom allocator ────────────────────────────────────────────

    /// Push a custom allocator onto the stack.
    pub fn push_custom_allocator(&mut self, allocator: Rc<AllocatorBox>) {
        self.custom_alloc_stack.push(CustomAllocState {
            allocator,
            custom_ptrs: Vec::new(),
            dtors: Vec::new(),
        });
    }

    /// Pop the top custom allocator, run Drop for remaining objects,
    /// then dealloc all remaining custom memory.
    pub fn pop_custom_allocator(&mut self) -> bool {
        let state = match self.custom_alloc_stack.pop() {
            Some(s) => s,
            None => return false,
        };

        // Run destructors for objects that need Drop.
        for &ptr in state.dtors.iter().rev() {
            if !ptr.is_null() {
                unsafe { std::ptr::drop_in_place(ptr) };
            }
        }

        // Dealloc all custom-allocated memory.
        for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
            state.allocator.inner.dealloc(ptr, size, align);
        }
        true
    }

    /// Check whether a shared allocator is active (legacy, always false).
    pub fn has_shared_alloc(&self) -> bool {
        false
    }

    // ── Teardown ────────────────────────────────────────────────────

    /// Drop all tracked objects and reset.
    pub fn clear(&mut self) {
        // Tear down custom allocators.
        for state in self.custom_alloc_stack.drain(..) {
            for &ptr in state.dtors.iter().rev() {
                if !ptr.is_null() {
                    unsafe { std::ptr::drop_in_place(ptr) };
                }
            }
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }

        // Tear down all physical regions.
        self.region_store.teardown_all();

        self.alloc_error = None;
        self.alloc_count = 0;
        self.peak_alloc_count = 0;
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        // Tear down physical regions first (run dtors, return pages).
        self.region_store.teardown_all();

        // Tear down custom allocators.
        for state in self.custom_alloc_stack.drain(..) {
            for &ptr in state.dtors.iter().rev() {
                if !ptr.is_null() {
                    unsafe { std::ptr::drop_in_place(ptr) };
                }
            }
            for &(ptr, size, align) in state.custom_ptrs.iter().rev() {
                state.allocator.inner.dealloc(ptr, size, align);
            }
        }
    }
}

impl Default for FiberHeap {
    fn default() -> Self {
        Self::new()
    }
}

/// Exhaustive check: does this HeapObject variant have inner heap allocations
/// that require Drop? No wildcard arm — adding a new HeapObject variant
/// forces a decision here (compile error).
pub(crate) fn needs_drop(tag: HeapTag) -> bool {
    match tag {
        HeapTag::Pair => false,
        HeapTag::LBox => true,
        HeapTag::CaptureCell => true,
        HeapTag::Float => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::LString => true,
        HeapTag::LArrayMut => true,
        HeapTag::LStructMut => true,
        HeapTag::LStruct => true,
        HeapTag::Closure => true,
        HeapTag::LArray => true,
        HeapTag::LStringMut => true,
        HeapTag::LBytes => true,
        HeapTag::LBytesMut => true,
        HeapTag::Syntax => true,
        HeapTag::Fiber => true,
        HeapTag::ThreadHandle => true,
        HeapTag::FFISignature => true,
        HeapTag::FFIType => true,
        HeapTag::External => true,
        HeapTag::Parameter => false,
        HeapTag::LSet => true,
        HeapTag::LSetMut => true,
        // Holds Rc fields (bytecode, constants, child_protos, …) that must be
        // dropped when the region frees.
        HeapTag::ClosureTemplate => true,
    }
}

/// Does this non-dtor HeapObject variant hold Value references that
/// need cascade decref on region free?
pub(crate) fn holds_value_refs(tag: HeapTag) -> bool {
    match tag {
        HeapTag::Pair => true,
        HeapTag::Parameter => true,
        HeapTag::Float => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::LBox
        | HeapTag::CaptureCell
        | HeapTag::LString
        | HeapTag::LArrayMut
        | HeapTag::LStructMut
        | HeapTag::LStruct
        | HeapTag::Closure
        | HeapTag::LArray
        | HeapTag::LStringMut
        | HeapTag::LBytes
        | HeapTag::LBytesMut
        | HeapTag::Syntax
        | HeapTag::Fiber
        | HeapTag::ThreadHandle
        | HeapTag::FFISignature
        | HeapTag::FFIType
        | HeapTag::External
        | HeapTag::LSet
        | HeapTag::LSetMut
        // ClosureTemplate is a dtor variant (needs_drop), so its cross-region
        // refs cascade through the `dtors` walk; this non-dtor predicate is false.
        | HeapTag::ClosureTemplate => false,
    }
}

#[cfg(test)]
mod tests;
