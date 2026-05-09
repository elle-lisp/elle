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
//!
//! ## Allocation tracking
//!
//! Live allocations are tracked via an intrusive doubly-linked list
//! threaded through the slab's `alloc_prev`/`alloc_next` parallel arrays.
//! `alloc_head` and `alloc_tail` are the list endpoints. This replaces the
//! former `Vec<*mut HeapObject>` — unlinking is O(1), so DropSlot no
//! longer leaves stale entries that cause double-free on release().

use super::bump::{BumpArena, BumpMark};
use super::needs_drop;
use super::slab::{Slab, ALLOC_NIL};
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Position snapshot for mark/release within a `SlabPool`.
#[derive(Clone)]
pub(crate) struct SlabMark {
    pub(crate) alloc_tail: u32,
    pub(crate) dtor_len: usize,
    pub(crate) alloc_count: usize,
    /// Bump arena position at mark time; used to reset for inline data.
    pub(crate) arena_mark: BumpMark,
}

pub(crate) struct SlabPool {
    pub(crate) slab: Slab,
    arena: BumpArena,
    /// Head of the allocation-order doubly-linked list (oldest allocation).
    pub(crate) alloc_head: u32,
    /// Tail of the allocation-order doubly-linked list (newest allocation).
    pub(crate) alloc_tail: u32,
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
            alloc_head: ALLOC_NIL,
            alloc_tail: ALLOC_NIL,
            dtors: Vec::new(),
            alloc_count: 0,
        }
    }

    // ── Linked list operations ──────────────────────────────────────

    /// Link a flat slot index at the tail of the allocation list.
    fn link_alloc_tail(&mut self, flat: u32) {
        let fi = flat as usize;
        self.slab.alloc_prev[fi] = self.alloc_tail;
        self.slab.alloc_next[fi] = ALLOC_NIL;
        if self.alloc_tail != ALLOC_NIL {
            self.slab.alloc_next[self.alloc_tail as usize] = flat;
        } else {
            self.alloc_head = flat;
        }
        self.alloc_tail = flat;
    }

    /// Unlink a flat slot index from the allocation list.
    pub fn unlink_alloc(&mut self, flat: u32) {
        let fi = flat as usize;
        let prev = self.slab.alloc_prev[fi];
        let next = self.slab.alloc_next[fi];
        if prev != ALLOC_NIL {
            self.slab.alloc_next[prev as usize] = next;
        } else {
            self.alloc_head = next;
        }
        if next != ALLOC_NIL {
            self.slab.alloc_prev[next as usize] = prev;
        } else {
            self.alloc_tail = prev;
        }
        // Clear the links on the unlinked node.
        self.slab.alloc_prev[fi] = ALLOC_NIL;
        self.slab.alloc_next[fi] = ALLOC_NIL;
    }

    /// Unlink a pointer from the allocation list by computing its flat index.
    pub fn unlink_alloc_ptr(&mut self, ptr: *mut HeapObject) {
        let flat = self.slab.ptr_to_flat(ptr) as u32;
        self.unlink_alloc(flat);
    }

    // ── Allocation ──────────────────────────────────────────────────

    /// Allocate a `HeapObject` into the slab, track it, and return a Value.
    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let value_tag = obj.value_tag();
        let drop = needs_drop(obj.tag());
        let ptr = self.slab.alloc(obj);
        let flat = self.slab.ptr_to_flat(ptr) as u32;
        self.link_alloc_tail(flat);
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
            alloc_tail: self.alloc_tail,
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
    /// slots to the free list, reset the bump arena.
    ///
    /// Walks the linked list from mark.alloc_tail.next to self.alloc_tail,
    /// unlinking and deallocating each node.
    pub fn release(&mut self, mark: &SlabMark) {
        self.run_dtors(mark.dtor_len);
        self.dtors.truncate(mark.dtor_len);

        // Walk the linked list from mark.alloc_tail.next to current tail.
        let start = if mark.alloc_tail == ALLOC_NIL {
            self.alloc_head
        } else {
            self.slab.alloc_next[mark.alloc_tail as usize]
        };
        let mut cur = start;
        while cur != ALLOC_NIL {
            let next = self.slab.alloc_next[cur as usize];
            let ptr = self.slab.flat_to_ptr(cur as usize);
            self.slab.dealloc(ptr);
            cur = next;
        }
        // Truncate the list at the mark point.
        self.alloc_tail = mark.alloc_tail;
        if self.alloc_tail == ALLOC_NIL {
            self.alloc_head = ALLOC_NIL;
        } else {
            self.slab.alloc_next[self.alloc_tail as usize] = ALLOC_NIL;
        }

        // Rewind bump arena for inline data.
        self.arena.release_to(mark.arena_mark);
        self.alloc_count = mark.alloc_count;
    }

    /// Run all destructors and reset both allocators.
    pub fn teardown(&mut self) {
        self.run_dtors(0);
        self.dtors.clear();
        self.alloc_head = ALLOC_NIL;
        self.alloc_tail = ALLOC_NIL;
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

    /// Remove a pointer from pool.dtors (O(n) scan).
    /// Called by drop_slot_value. The allocs list is handled by unlink_alloc_ptr.
    pub fn remove_from_dtors(&mut self, ptr: *mut HeapObject) {
        self.dtors.retain(|&p| p != ptr);
    }

    /// Decref a value if it's owned by this pool's slab.
    /// Used when freeing a parent object to decref its children.
    pub fn decref_if_owned(&mut self, val: Value) {
        if !val.is_heap() {
            return;
        }
        if let Some(ptr) = val.as_heap_ptr() {
            if self.slab.owns(ptr) {
                let old_rc = self.slab.refcount(ptr as *const HeapObject);
                if old_rc > 0 {
                    self.slab.decref(ptr as *const HeapObject);
                }
            }
        }
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

    /// Rewind the bump arena to a saved position, freeing pages after the mark.
    ///
    /// Called by `FiberHeap::release()` and friends to reclaim inline data
    /// (strings, arrays, bytes) allocated after the scope mark.
    pub fn release_bump_to(&mut self, mark: BumpMark) {
        self.arena.release_to(mark);
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
    /// The caller must have run all destructors and cleared `dtors`
    /// before calling this.
    pub unsafe fn clear_slab(&mut self) {
        self.alloc_head = ALLOC_NIL;
        self.alloc_tail = ALLOC_NIL;
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
