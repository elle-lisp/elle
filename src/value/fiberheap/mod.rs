//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses `RegionStore` — a physical region allocator where
//! each region owns its pages exclusively. `FreeRegion(ρ)` tears down
//! the region's pages (with cascade decref for cross-region refs).

use std::rc::Rc;

use crate::value::allocator::AllocatorBox;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

mod routing;
pub use routing::*;

pub(crate) mod pagepool;
pub(crate) mod regionpool;
pub(crate) mod regionstore;

use regionstore::RegionStore;

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
        }
    }

    /// Object count (used by arena primitives and object limiting).
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    pub fn visible_len(&self) -> usize {
        self.alloc_count
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
    pub fn alloc_in_region(&mut self, obj: HeapObject, region_id: u16) -> Value {
        assert!(
            region_id != 0,
            "alloc_in_region called with region_id 0 — solver bug"
        );

        if let Some(limit) = self.object_limit {
            if self.alloc_count >= limit {
                self.alloc_error = Some((self.alloc_count, limit));
                return Value::NIL;
            }
        }

        let v = self.region_store.alloc_obj(region_id, obj);
        self.alloc_count += 1;
        if self.alloc_count > self.peak_alloc_count {
            self.peak_alloc_count = self.alloc_count;
        }
        v
    }

    /// Allocate an inline slice directly into a specific region.
    pub fn alloc_inline_slice_in_region<T: Copy + 'static>(
        &mut self,
        items: &[T],
        region_id: u16,
    ) -> crate::value::inline_slice::InlineSlice<T> {
        assert!(
            region_id != 0,
            "alloc_inline_slice_in_region called with region_id 0 — solver bug"
        );
        self.region_store.alloc_inline_slice(region_id, items)
    }

    /// Increment the reference count for a region.
    pub fn incref_region(&mut self, id: u16) {
        self.region_store.incref(id);
    }

    /// Decrement the reference count for a region.
    pub fn decref_region(&mut self, id: u16) {
        self.region_store.decref(id);
    }

    /// Free a region. Defers if the region's RC > 0.
    pub fn free_region_physical(&mut self, region: u16) {
        self.region_store.free_region(region);
    }

    /// Page size used by the region store's page pool.
    pub fn region_page_size(&self) -> usize {
        self.region_store.page_size()
    }

    /// Get the reference count for a region.
    pub fn region_rc(&self, id: u16) -> u32 {
        self.region_store.rc(id)
    }

    /// Number of active regions.
    pub fn active_region_count(&self) -> usize {
        self.region_store.active_region_count()
    }

    /// Per-region info: (region_id, rc, object_count) for every active region.
    pub fn region_info_vec(&self) -> Vec<(u16, u32, usize)> {
        self.region_store.region_info_vec()
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
        HeapTag::NativeFn => false,
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
    }
}

/// Does this non-dtor HeapObject variant hold Value references that
/// need cascade decref on region free?
pub(crate) fn holds_value_refs(tag: HeapTag) -> bool {
    match tag {
        HeapTag::Pair => true,
        HeapTag::Parameter => true,
        HeapTag::Float => false,
        HeapTag::NativeFn => false,
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
        | HeapTag::LSetMut => false,
    }
}

#[cfg(test)]
mod tests;
