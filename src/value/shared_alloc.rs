//! Shared slab allocator for zero-copy inter-fiber value exchange.
//!
//! `SharedAllocator` is owned by a parent fiber's `FiberHeap` (or by the
//! child itself for root->child chains). Child fibers receive a raw pointer
//! and allocate into it during execution. No Rc, no RefCell, no runtime
//! borrow checks on the allocation path.
//!
//! Lifecycle:
//! - Created by the parent's `FiberHeap::create_shared_allocator()`
//! - Child allocates fiber-escaping values into it via raw pointer
//! - Parent reads yielded values directly (zero copy)
//! - Torn down when the owner's `FiberHeap::clear()` runs;
//!   `teardown()` runs destructors and returns all slab slots to the free list

use crate::value::fiberheap::pool::{SlabMark, SlabPool};
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Saved position for scope-based release within a SharedAllocator.
struct SharedMark {
    slab: SlabMark,
}

/// Previous tail-call iteration's shared allocations, preserved for one
/// rotation cycle so argument values from iteration N remain valid until
/// iteration N+1 copies them.
///
/// Not yet activated — shared rotation requires reachability analysis
/// to avoid freeing objects referenced by live chains (e.g., cons lists
/// accumulated via tail-call arguments).
#[allow(dead_code)]
struct SharedSwapPool {
    allocs: Vec<*mut HeapObject>,
    dtors: Vec<*mut HeapObject>,
}

pub(crate) struct SharedAllocator {
    pool: SlabPool,
    /// Stack of scope marks for RegionEnter/RegionExit on child fibers.
    marks: Vec<SharedMark>,
    /// Swap pool for tail-call rotation.
    swap: Option<SharedSwapPool>,
}

impl SharedAllocator {
    pub fn new() -> Self {
        SharedAllocator {
            pool: SlabPool::new(),
            marks: Vec::new(),
            swap: None,
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        self.pool.alloc(obj)
    }

    /// Copy `items` into the shared pool's arena and return an InlineSlice.
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
    ) -> crate::value::inline_slice::InlineSlice<T> {
        self.pool.alloc_inline_slice(items)
    }

    /// Push a scope mark recording the current pool position.
    pub fn push_mark(&mut self) {
        self.marks.push(SharedMark {
            slab: self.pool.mark(),
        });
    }

    /// Pop the top scope mark and release objects allocated since it was pushed.
    pub fn pop_mark_and_release(&mut self) {
        let mark = self
            .marks
            .pop()
            .expect("SharedAllocator::pop_mark_and_release without matching push_mark");
        self.pool.release(&mark.slab);
    }

    /// Capture the current pool position for rotation.
    #[allow(dead_code)]
    pub(crate) fn rotation_mark(&self) -> SlabMark {
        self.pool.mark()
    }

    /// Clear the swap pool without freeing any slots. Used when a new
    /// trampoline captures a rotation mark: discard stale swap state
    /// from a previous child fiber so the new child's rotation doesn't
    /// tear down objects that the previous child still references.
    #[allow(dead_code)]
    pub(crate) fn clear_swap(&mut self) {
        if let Some(old) = self.swap.take() {
            // Return objects from the swap pool to the main pool tracking.
            // These objects are still live (they may be referenced by the
            // previous child fiber). Move them back so teardown handles them.
            self.pool.allocs.extend(old.allocs);
            self.pool.dtors.extend(old.dtors);
        }
    }

    /// Rotate the shared pool at a tail-call boundary.
    ///
    /// Same protocol as FiberHeap::rotate_pools:
    /// 1. Teardown swap (iteration N-2 is dead)
    /// 2. Move current iteration's objects to swap
    /// 3. Reset alloc_count to base level
    #[allow(dead_code)]
    pub(crate) fn rotate(&mut self, base: &SlabMark) {
        // 1. Teardown previous swap pool.
        if let Some(old) = self.swap.take() {
            for i in (0..old.dtors.len()).rev() {
                unsafe { std::ptr::drop_in_place(old.dtors[i]) };
            }
            for &ptr in old.allocs.iter().rev() {
                unsafe { self.pool.dealloc_slot(ptr) };
            }
        }

        // 2. Move current iteration's objects to swap.
        let iter_allocs = self.pool.allocs.split_off(base.allocs_len);
        let iter_dtors = self.pool.dtors.split_off(base.dtor_len);

        self.swap = if iter_allocs.is_empty() {
            None
        } else {
            Some(SharedSwapPool {
                allocs: iter_allocs,
                dtors: iter_dtors,
            })
        };

        // 3. Reset alloc_count to base level.
        self.pool.alloc_count = base.alloc_count;
    }

    /// Run destructors, return all slots to the slab free list, and reset.
    pub fn teardown(&mut self) {
        // Drain swap pool first.
        if let Some(old) = self.swap.take() {
            for i in (0..old.dtors.len()).rev() {
                unsafe { std::ptr::drop_in_place(old.dtors[i]) };
            }
            // Slab slots freed by pool.teardown() below.
        }
        self.pool.teardown();
        self.marks.clear();
    }

    /// Bytes committed by the shared slab.
    pub fn allocated_bytes(&self) -> usize {
        self.pool.allocated_bytes()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }
}

impl Default for SharedAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{HeapObject, Pair};

    /// Allocate an `LString` HeapObject with its bytes inline in `sa`'s arena.
    fn alloc_str(sa: &mut SharedAllocator, text: &str) -> Value {
        let s = sa.alloc_inline_slice::<u8>(text.as_bytes());
        sa.alloc(HeapObject::LString {
            s,
            traits: Value::NIL,
        })
    }

    #[test]
    fn test_shared_alloc_basic() {
        let mut sa = SharedAllocator::new();
        let v = alloc_str(&mut sa, "hello");
        assert_eq!(sa.len(), 1);
        assert!(v.is_heap());
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::LString { s, .. } => assert_eq!(s.as_slice(), b"hello"),
                _ => panic!("Expected String"),
            }
        }
    }

    #[test]
    fn test_shared_alloc_no_drop_types() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 1);
        assert_eq!(sa.pool.allocs.len(), 1);
    }

    #[test]
    fn test_shared_alloc_drop_types_tracked() {
        // Post-Phase-2: LString no longer owns a Box<str>, so it doesn't need
        // Drop tracking. This test verifies `len()` still counts all live
        // objects and the alloc list records every pointer.
        let mut sa = SharedAllocator::new();
        alloc_str(&mut sa, "tracked");
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 2);
        assert_eq!(sa.pool.allocs.len(), 2);
    }

    #[test]
    fn test_shared_alloc_teardown_runs_dtors() {
        let mut sa = SharedAllocator::new();
        alloc_str(&mut sa, "a");
        alloc_str(&mut sa, "b");
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 3);

        sa.teardown();
        assert_eq!(sa.len(), 0);
        assert!(sa.is_empty());
        assert_eq!(sa.pool.allocs.len(), 0);
    }

    #[test]
    fn test_shared_alloc_teardown_reuses_memory() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        alloc_str(&mut sa, "x");
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.pool.live_count(), 3);
        sa.teardown();
        assert_eq!(sa.pool.live_count(), 0);
        assert_eq!(sa.len(), 0);

        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        alloc_str(&mut sa, "y");
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.pool.live_count(), 3);
        let bytes_round1 = sa.pool.allocated_bytes();
        sa.teardown();

        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(
            sa.pool.allocated_bytes(),
            bytes_round1,
            "slab must reuse freed slots, not allocate new chunks"
        );
        sa.teardown();
    }

    #[test]
    fn test_shared_alloc_len() {
        let mut sa = SharedAllocator::new();
        assert!(sa.is_empty());
        assert_eq!(sa.len(), 0);

        sa.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 1);
        assert!(!sa.is_empty());

        alloc_str(&mut sa, "x");
        assert_eq!(sa.len(), 2);

        sa.alloc(HeapObject::Pair(Pair::new(Value::TRUE, Value::EMPTY_LIST)));
        assert_eq!(sa.len(), 3);
    }
}
