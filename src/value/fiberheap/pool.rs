//! Common slab pool: slab allocator + allocation tracking + destructor list.
//!
//! `SlabPool` is the shared core of `FiberHeap` and `SharedAllocator`.
//! It owns a `RootSlab`, tracks all allocations for mark/release, and
//! maintains a destructor list for `HeapObject` variants with inner heap
//! data.

use super::needs_drop;
use super::slab::RootSlab;
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Position snapshot for mark/release within a `SlabPool`.
pub(crate) struct SlabMark {
    pub(crate) allocs_len: usize,
    pub(crate) dtor_len: usize,
    pub(crate) alloc_count: usize,
}

pub(crate) struct SlabPool {
    slab: RootSlab,
    /// All slab allocations, in allocation order.
    /// Used by `release()` to dealloc slots allocated after a mark.
    pub(crate) allocs: Vec<*mut HeapObject>,
    /// Raw pointers to HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    pub(crate) dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    pub(crate) alloc_count: usize,
}

impl SlabPool {
    pub fn new() -> Self {
        SlabPool {
            slab: RootSlab::new(),
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

    /// Capture the current position for later release.
    pub fn mark(&self) -> SlabMark {
        SlabMark {
            allocs_len: self.allocs.len(),
            dtor_len: self.dtors.len(),
            alloc_count: self.alloc_count,
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
    /// slots to the free list, truncate tracking vecs.
    pub fn release(&mut self, mark: &SlabMark) {
        self.run_dtors(mark.dtor_len);
        self.dtors.truncate(mark.dtor_len);
        for &ptr in self.allocs[mark.allocs_len..].iter().rev() {
            self.slab.dealloc(ptr);
        }
        self.allocs.truncate(mark.allocs_len);
        self.alloc_count = mark.alloc_count;
    }

    /// Run all destructors, return all slots to free list, reset slab.
    pub fn teardown(&mut self) {
        self.run_dtors(0);
        self.dtors.clear();
        for &ptr in self.allocs.iter().rev() {
            self.slab.dealloc(ptr);
        }
        self.allocs.clear();
        self.slab.clear();
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
        self.slab.allocated_bytes()
    }

    pub fn capacity_bytes(&self) -> usize {
        self.slab.capacity_bytes()
    }

    /// Return a slab slot to the free list.
    ///
    /// # Safety
    /// The caller must have already run `drop_in_place(ptr)` if needed.
    /// `ptr` must have been returned by a prior `alloc()` on this pool.
    pub unsafe fn dealloc_slot(&mut self, ptr: *mut HeapObject) {
        self.slab.dealloc(ptr);
    }

    /// Reset the slab (keeps first chunk). Does NOT run destructors or
    /// clear tracking vecs — caller must handle those first.
    ///
    /// # Safety
    /// The caller must have run all destructors and cleared `dtors`/`allocs`
    /// before calling this.
    pub unsafe fn clear_slab(&mut self) {
        self.slab.clear();
    }
}

impl Drop for SlabPool {
    fn drop(&mut self) {
        // Run destructors before the slab drops.
        self.run_dtors(0);
        // slab drops implicitly; MaybeUninit slots do not call HeapObject::drop.
    }
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new()
    }
}
