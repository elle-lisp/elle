//! Shared slab allocator for zero-copy inter-fiber value exchange.
//!
//! `SharedAllocator` is owned by a parent fiber's `FiberHeap` (or by the
//! child itself for root→child chains). Child fibers receive a raw pointer
//! and allocate into it during execution. No Rc, no RefCell, no runtime
//! borrow checks on the allocation path.
//!
//! Lifecycle:
//! - Created by the parent's `FiberHeap::create_shared_allocator()`
//! - Child allocates fiber-escaping values into it via raw pointer
//! - Parent reads yielded values directly (zero copy)
//! - Torn down when the owner's `FiberHeap::clear()` runs;
//!   `teardown()` runs destructors and returns all slab slots to the free list

use crate::value::fiber_heap::needs_drop;
use crate::value::fiber_heap::RootSlab;
use crate::value::heap::HeapObject;
use crate::value::Value;

pub(crate) struct SharedAllocator {
    slab: RootSlab,
    /// Raw pointers to slab-allocated HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// ALL slab allocations (not just drop-needing ones).
    /// Used by `teardown()` and `Drop` to return every slot to the free list.
    allocs: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
}

impl SharedAllocator {
    pub fn new() -> Self {
        SharedAllocator {
            slab: RootSlab::new(),
            dtors: Vec::new(),
            allocs: Vec::new(),
            alloc_count: 0,
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let drop = needs_drop(obj.tag());
        let ptr = self.slab.alloc(obj);
        self.allocs.push(ptr);
        if drop {
            self.dtors.push(ptr);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(ptr as *const ())
    }

    /// Run destructors, return all slots to the slab free list, and reset.
    pub fn teardown(&mut self) {
        // Run destructors in reverse order (LIFO).
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
        self.dtors.clear();
        // Return all allocated slots to the slab free list.
        // Dtors have already run — safe to dealloc.
        for &ptr in self.allocs.iter().rev() {
            self.slab.dealloc(ptr);
        }
        self.allocs.clear();
        self.slab.clear();
        self.alloc_count = 0;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }
}

impl Drop for SharedAllocator {
    fn drop(&mut self) {
        // Run destructors before the slab drops.
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
        // slab drops implicitly; MaybeUninit slots do not call HeapObject::drop.
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
        assert_eq!(sa.dtors.len(), 0);
        assert_eq!(sa.allocs.len(), 1);
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
        assert_eq!(sa.dtors.len(), 1); // only String
        assert_eq!(sa.allocs.len(), 2);
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
        assert_eq!(sa.dtors.len(), 2);

        sa.teardown();
        assert_eq!(sa.len(), 0);
        assert!(sa.is_empty());
        assert_eq!(sa.dtors.len(), 0);
        assert_eq!(sa.allocs.len(), 0);
    }

    #[test]
    fn test_shared_alloc_teardown_reuses_memory() {
        let mut sa = SharedAllocator::new();
        // Alloc 3 objects and teardown.
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::LString {
            s: "x".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        let live_after_first = sa.slab.live_count();
        assert_eq!(live_after_first, 3);
        sa.teardown();
        assert_eq!(sa.slab.live_count(), 0);
        assert_eq!(sa.len(), 0);

        // Alloc again — slab must reuse slots (live_count stays at 3 after 3 allocs).
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::LString {
            s: "y".into(),
            traits: Value::NIL,
        });
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.slab.live_count(), 3);
        // Allocated bytes must not have grown (slots were reused).
        // With chunk_size=256, one chunk covers 3 objects easily.
        let bytes_round1 = sa.slab.allocated_bytes();
        sa.teardown();

        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(
            sa.slab.allocated_bytes(),
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

        sa.alloc(HeapObject::Float(42.5));
        assert_eq!(sa.len(), 3);
    }
}
