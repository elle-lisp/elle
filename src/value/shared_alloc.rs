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

use crate::value::fiberheap::pool::SlabPool;
use crate::value::heap::HeapObject;
use crate::value::Value;

/// Saved position for scope-based release within a SharedAllocator.
struct SharedMark {
    slab: crate::value::fiberheap::pool::SlabMark,
}

pub(crate) struct SharedAllocator {
    pool: SlabPool,
    /// Stack of scope marks for RegionEnter/RegionExit on child fibers.
    marks: Vec<SharedMark>,
}

impl SharedAllocator {
    pub fn new() -> Self {
        SharedAllocator {
            pool: SlabPool::new(),
            marks: Vec::new(),
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        self.pool.alloc(obj)
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

    /// Run destructors, return all slots to the slab free list, and reset.
    pub fn teardown(&mut self) {
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
    use crate::value::heap::{Cons, HeapObject};

    #[test]
    fn test_shared_alloc_basic() {
        let mut sa = SharedAllocator::new();
        let v = sa.alloc(HeapObject::LString {
            s: "hello".into(),
            traits: Value::NIL,
        });
        assert_eq!(sa.len(), 1);
        assert!(v.is_heap());
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::LString { s, .. } => assert_eq!(&**s, "hello"),
                _ => panic!("Expected String"),
            }
        }
    }

    #[test]
    fn test_shared_alloc_no_drop_types() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 1);
        assert_eq!(sa.pool.dtor_count(), 0);
        assert_eq!(sa.pool.allocs.len(), 1);
    }

    #[test]
    fn test_shared_alloc_drop_types_tracked() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::LString {
            s: "tracked".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 2);
        assert_eq!(sa.pool.dtor_count(), 1); // only String
        assert_eq!(sa.pool.allocs.len(), 2);
    }

    #[test]
    fn test_shared_alloc_teardown_runs_dtors() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::LString {
            s: "a".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::LString {
            s: "b".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 3);
        assert_eq!(sa.pool.dtor_count(), 2);

        sa.teardown();
        assert_eq!(sa.len(), 0);
        assert!(sa.is_empty());
        assert_eq!(sa.pool.dtor_count(), 0);
        assert_eq!(sa.pool.allocs.len(), 0);
    }

    #[test]
    fn test_shared_alloc_teardown_reuses_memory() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::LString {
            s: "x".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.pool.live_count(), 3);
        sa.teardown();
        assert_eq!(sa.pool.live_count(), 0);
        assert_eq!(sa.len(), 0);

        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::LString {
            s: "y".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.pool.live_count(), 3);
        let bytes_round1 = sa.pool.allocated_bytes();
        sa.teardown();

        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
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

        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 1);
        assert!(!sa.is_empty());

        sa.alloc(HeapObject::LString {
            s: "x".into(),
            traits: Value::NIL,
        });
        assert_eq!(sa.len(), 2);

        sa.alloc(HeapObject::Cons(Cons::new(Value::TRUE, Value::EMPTY_LIST)));
        assert_eq!(sa.len(), 3);
    }
}
