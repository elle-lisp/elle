//! Shared bump allocator for zero-copy inter-fiber value exchange.
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
//! - Torn down when the owner's `FiberHeap::clear()` runs

use crate::value::fiber_heap::needs_drop;
use crate::value::heap::HeapObject;
use crate::value::Value;

pub(crate) struct SharedAllocator {
    bump: bumpalo::Bump,
    /// Raw pointers to bump-allocated HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
}

impl SharedAllocator {
    pub fn new() -> Self {
        SharedAllocator {
            bump: bumpalo::Bump::new(),
            dtors: Vec::new(),
            alloc_count: 0,
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let drop = needs_drop(obj.tag());
        let ptr: &mut HeapObject = self.bump.alloc(obj);
        let raw = ptr as *mut HeapObject;
        if drop {
            self.dtors.push(raw);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(raw as *const ())
    }

    /// Run destructors, clear tracking, and reset the bump allocator.
    pub fn teardown(&mut self) {
        // Run destructors in reverse order (LIFO).
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
        self.dtors.clear();
        self.alloc_count = 0;
        self.bump.reset();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    /// Access the underlying bump allocator.
    /// Needed for future `active_allocator` tightening phases.
    #[allow(dead_code)]
    pub fn bump(&self) -> &bumpalo::Bump {
        &self.bump
    }
}

impl Drop for SharedAllocator {
    fn drop(&mut self) {
        // Run destructors before the bump deallocates its memory.
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
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
        let v = sa.alloc(HeapObject::LString("hello".into()));
        assert_eq!(sa.len(), 1);
        assert!(v.is_heap());
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::LString(s) => assert_eq!(&**s, "hello"),
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
    }

    #[test]
    fn test_shared_alloc_drop_types_tracked() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::LString("tracked".into()));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 2);
        assert_eq!(sa.dtors.len(), 1); // only String
    }

    #[test]
    fn test_shared_alloc_teardown_runs_dtors() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::LString("a".into()));
        sa.alloc(HeapObject::LString("b".into()));
        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 3);
        assert_eq!(sa.dtors.len(), 2);

        sa.teardown();
        assert_eq!(sa.len(), 0);
        assert!(sa.is_empty());
        assert_eq!(sa.dtors.len(), 0);
    }

    #[test]
    fn test_shared_alloc_teardown_resets_bump() {
        let mut sa = SharedAllocator::new();
        sa.alloc(HeapObject::LString("first".into()));
        sa.teardown();

        // Allocate again after teardown — should work fine
        let v = sa.alloc(HeapObject::LString("second".into()));
        assert_eq!(sa.len(), 1);
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::LString(s) => assert_eq!(&**s, "second"),
                _ => panic!("Expected String"),
            }
        }
    }

    #[test]
    fn test_shared_alloc_len() {
        let mut sa = SharedAllocator::new();
        assert!(sa.is_empty());
        assert_eq!(sa.len(), 0);

        sa.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(sa.len(), 1);
        assert!(!sa.is_empty());

        sa.alloc(HeapObject::LString("x".into()));
        assert_eq!(sa.len(), 2);

        sa.alloc(HeapObject::Float(42.5));
        assert_eq!(sa.len(), 3);
    }
}
