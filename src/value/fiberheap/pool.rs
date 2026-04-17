//! Common allocation pool: bump arena + destructor tracking.
//!
//! `SlabPool` is the shared core of `FiberHeap` and `SharedAllocator`.
//! After the Phase 1 arena migration, the storage backend is a `BumpArena`
//! (pages of `HeapObject`-sized slots) rather than a slab with free list.
//!
//! Individual-slot deallocation is no longer supported; memory is reclaimed
//! only by scope-based `release(mark)` or `teardown()`. The `dealloc_slot`
//! method is retained as a no-op compatibility shim for rotation paths that
//! will be redesigned in Phase 4.

use super::bump::{BumpArena, BumpMark};
use super::needs_drop;
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Position snapshot for mark/release within a `SlabPool`.
#[derive(Clone)]
pub(crate) struct SlabMark {
    pub(crate) allocs_len: usize,
    pub(crate) dtor_len: usize,
    pub(crate) alloc_count: usize,
    /// Arena position at mark time; used to reset the bump pointer.
    pub(crate) arena_mark: BumpMark,
}

pub(crate) struct SlabPool {
    arena: BumpArena,
    /// Every allocation's pointer, in allocation order. Retained for
    /// compatibility with rotation/release paths that inspect allocation
    /// order. The arena itself is the source of truth for memory layout.
    pub(crate) allocs: Vec<*mut HeapObject>,
    /// Pointers to HeapObjects that need Drop, in allocation order.
    pub(crate) dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated since creation/clear.
    pub(crate) alloc_count: usize,
}

impl SlabPool {
    pub fn new() -> Self {
        SlabPool {
            arena: BumpArena::new(),
            allocs: Vec::new(),
            dtors: Vec::new(),
            alloc_count: 0,
        }
    }

    /// Allocate a `HeapObject` into the arena, track it, and return a Value.
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let value_tag = obj.value_tag();
        let drop = needs_drop(obj.tag());
        let ptr = self.arena.alloc(obj);
        self.allocs.push(ptr);
        if drop {
            self.dtors.push(ptr);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(ptr as *const (), value_tag)
    }

    /// Copy `items` into the arena and return an `InlineSlice` pointing to them.
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

    /// Release allocations back to a mark: run destructors, reset the
    /// arena bump pointer, and truncate tracking vecs.
    pub fn release(&mut self, mark: &SlabMark) {
        self.run_dtors(mark.dtor_len);
        self.dtors.truncate(mark.dtor_len);
        self.allocs.truncate(mark.allocs_len);
        self.arena.release_to(mark.arena_mark);
        self.alloc_count = mark.alloc_count;
    }

    /// Run all destructors and reset the arena.
    pub fn teardown(&mut self) {
        self.run_dtors(0);
        self.dtors.clear();
        self.allocs.clear();
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
        // With a bump arena, live count equals the running alloc count.
        // (Rotation/drop paths decrement alloc_count manually.)
        self.alloc_count
    }

    pub fn allocated_bytes(&self) -> usize {
        self.arena.allocated_bytes()
    }

    pub fn capacity_bytes(&self) -> usize {
        self.arena.allocated_bytes()
    }

    /// Compatibility shim for rotation paths. In the slab model, this
    /// returned a slot to the free list. In the arena model, individual
    /// slots cannot be reclaimed — memory is only freed by `release()` or
    /// `teardown()`. Phase 4 will replace the swap-pool rotation entirely
    /// with double-buffered arena swap.
    ///
    /// # Safety
    /// The caller must have already run `drop_in_place(ptr)` if needed.
    /// `ptr` must have been returned by a prior `alloc()` on this pool.
    pub unsafe fn dealloc_slot(&mut self, _ptr: *mut HeapObject) {
        // No-op under the bump-arena model. Memory is reclaimed by the
        // enclosing release/teardown. See module docs and Phase 4 plan.
    }

    /// Check if a pointer falls within this pool's arena pages.
    pub fn owns(&self, ptr: *const ()) -> bool {
        self.arena.owns(ptr)
    }

    /// Reset the arena (keep one page). Does NOT run destructors or
    /// clear tracking vecs — caller must handle those first.
    ///
    /// # Safety
    /// The caller must have run all destructors and cleared `dtors`/`allocs`
    /// before calling this.
    pub unsafe fn clear_slab(&mut self) {
        self.arena.clear();
    }
}

impl Drop for SlabPool {
    fn drop(&mut self) {
        // Run destructors before the arena drops.
        self.run_dtors(0);
    }
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new()
    }
}
