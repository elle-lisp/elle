//! Common allocation pool: slab for HeapObjects + bump arena for inline data.
//!
//! `SlabPool` is the shared core of `FiberHeap` and `SharedAllocator`.
//! It uses two allocators:
//!
//! - **Slab** (`Slab`): fixed-size `HeapObject` slots with a free list.
//!   Supports individual slot deallocation (for drop-on-overwrite) and
//!   batch return on scope exit. Backed by mmap'd chunks.
//!
//! - **Bump arena** (`BumpArena`): variable-size data for `InlineSlice`
//!   payloads attached to HeapObjects. Reclaimed in bulk by scope marks
//!   or teardown. Backed by mmap'd pages.

use super::bump::{BumpArena, BumpMark};
use super::needs_drop;
use super::slab::Slab;
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Position snapshot for mark/release within a `SlabPool`.
#[derive(Clone)]
pub(crate) struct SlabMark {
    pub(crate) allocs_len: usize,
    pub(crate) dtor_len: usize,
    pub(crate) alloc_count: usize,
    /// Bump arena position at mark time; used to reset for inline data.
    pub(crate) arena_mark: BumpMark,
}

pub(crate) struct SlabPool {
    slab: Slab,
    arena: BumpArena,
    /// Every allocation's pointer, in allocation order. Used by rotation
    /// and scope-release paths that inspect allocation order.
    pub(crate) allocs: Vec<*mut HeapObject>,
    /// Pointers to HeapObjects that need Drop, in allocation order.
    pub(crate) dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated since creation/clear.
    pub(crate) alloc_count: usize,
}

impl SlabPool {
    pub fn new() -> Self {
        SlabPool {
            slab: Slab::new(),
            arena: BumpArena::new(),
            allocs: Vec::new(),
            dtors: Vec::new(),
            alloc_count: 0,
        }
    }

    /// Allocate a `HeapObject` into the slab, track it, and return a Value.
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let value_tag = obj.value_tag();
        let drop = needs_drop(obj.tag());
        let ptr = self.slab.alloc(obj);
        self.allocs.push(ptr);
        if drop {
            self.dtors.push(ptr);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(ptr as *const (), value_tag)
    }

    /// Copy `items` into the bump arena and return an `InlineSlice`.
    /// Inline-slice allocations don't count against `alloc_count` — they're
    /// data buffers attached to a `HeapObject` rather than standalone objects.
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
    ) -> crate::value::inline_slice::InlineSlice<T> {
        if items.is_empty() {
            return crate::value::inline_slice::InlineSlice::empty();
        }
        let ptr = self.arena.alloc_slice(items);
        unsafe { crate::value::inline_slice::InlineSlice::from_raw(ptr, items.len() as u32) }
    }

    /// Capture the current position for later release.
    pub fn mark(&self) -> SlabMark {
        SlabMark {
            allocs_len: self.allocs.len(),
            dtor_len: self.dtors.len(),
            alloc_count: self.alloc_count,
            arena_mark: self.arena.mark(),
        }
    }

    /// Run destructors in reverse order from `self.dtors[start..]`.
    pub fn run_dtors(&self, start: usize) {
        for i in (start..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
    }

    /// Release allocations back to a mark: run destructors, return slab
    /// slots to the free list, reset the bump arena, truncate tracking vecs.
    pub fn release(&mut self, mark: &SlabMark) {
        self.run_dtors(mark.dtor_len);
        self.dtors.truncate(mark.dtor_len);

        // Return slab slots to the free list.
        for i in (mark.allocs_len..self.allocs.len()).rev() {
            // SAFETY: run_dtors already ran destructors; slots are safe to free.
            self.slab.dealloc(self.allocs[i]);
        }
        self.allocs.truncate(mark.allocs_len);

        // Rewind bump arena for inline data.
        self.arena.release_to(mark.arena_mark);
        self.alloc_count = mark.alloc_count;
    }

    /// Run all destructors and reset both allocators.
    pub fn teardown(&mut self) {
        self.run_dtors(0);
        self.dtors.clear();
        self.allocs.clear();
        self.slab.clear();
        self.arena.clear();
        self.alloc_count = 0;
    }

    pub fn len(&self) -> usize {
        self.alloc_count
    }

    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    pub fn dtor_count(&self) -> usize {
        self.dtors.len()
    }

    pub fn live_count(&self) -> usize {
        self.slab.live_count()
    }

    pub fn allocated_bytes(&self) -> usize {
        self.slab.allocated_bytes() + self.arena.allocated_bytes()
    }

    pub fn capacity_bytes(&self) -> usize {
        self.allocated_bytes()
    }

    /// Return a slab slot to the free list for reuse by a future allocation.
    ///
    /// Called by RegionExit paths (`FiberHeap::release`,
    /// `pop_call_scope_marks_and_release`) which are gated by Tofte-Talpin
    /// escape analysis — the analysis proves no live values reference these
    /// slots before the call.
    ///
    /// # Safety
    /// The caller must have already called `drop_in_place(ptr)` if the object
    /// needs Drop. `ptr` must have been returned by a prior `alloc()` on this
    /// pool and must not have been deallocated since. No live `Value` may
    /// reference this slot after this call.
    #[inline]
    pub unsafe fn dealloc_slot(&mut self, ptr: *mut HeapObject) {
        self.slab.dealloc(ptr);
    }

    /// Deferred slot deallocation for rotation paths.
    ///
    /// Rotation (`rotate_pools`, `flip_swap`, `flip_exit`) moves objects to a
    /// swap pool and frees them one iteration later. The one-iteration lag
    /// is intended to keep argument values alive, but the temporal partitioning
    /// is incorrect: some objects allocated after the rotation mark survive
    /// across iterations (returned values, closures, mutable bindings).
    ///
    /// Until the rotation partitioning is fixed (Phase 2A), slot deallocation
    /// in rotation paths remains disabled. Slots are reclaimed only on fiber
    /// death (teardown/clear), which is where the mmap-backed pages return to
    /// the OS.
    ///
    /// # Safety
    /// Same contract as `dealloc_slot`. Currently a no-op.
    #[inline]
    pub unsafe fn dealloc_slot_deferred(&mut self, _ptr: *mut HeapObject) {
        // TODO: Enable `self.slab.dealloc(ptr)` after rotation partitioning fix.
    }

    // ── Refcounting ───────────────────────────────────────────────────

    /// Increment the durable reference count for a slab slot.
    #[inline]
    pub fn incref(&mut self, ptr: *const HeapObject) {
        self.slab.incref(ptr);
    }

    /// Decrement the durable reference count. Returns the new refcount.
    #[inline]
    pub fn decref(&mut self, ptr: *const HeapObject) -> u32 {
        self.slab.decref(ptr)
    }

    /// Get the durable reference count for a slab slot.
    #[inline]
    pub fn refcount(&self, ptr: *const HeapObject) -> u32 {
        self.slab.refcount(ptr)
    }

    /// Check if a pointer is in the slab (not arena).
    pub fn slab_owns(&self, ptr: *const ()) -> bool {
        self.slab.owns(ptr)
    }

    /// Check if a pointer falls within this pool's slab chunks or arena pages.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.slab.owns(ptr) || self.arena.owns(ptr)
    }

    /// Reset both allocators. Does NOT run destructors or clear tracking
    /// vecs — caller must handle those first.
    ///
    /// # Safety
    /// The caller must have run all destructors and cleared `dtors`/`allocs`
    /// before calling this.
    pub unsafe fn clear_slab(&mut self) {
        self.slab.clear();
        self.arena.clear();
    }
}

impl Drop for SlabPool {
    fn drop(&mut self) {
        // Run destructors before the allocators drop.
        self.run_dtors(0);
    }
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new()
    }
}
