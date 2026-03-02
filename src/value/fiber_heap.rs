//! Per-fiber heap ownership and thread-local current-heap routing.

use std::cell::Cell;

use crate::value::heap::{ArenaMark, HeapObject};
use crate::value::Value;

pub struct FiberHeap {
    #[allow(clippy::vec_box)]
    objects: Vec<Box<HeapObject>>,
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            objects: Vec::new(),
        }
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let boxed = Box::new(obj);
        let ptr = &*boxed as *const HeapObject as *const ();
        self.objects.push(boxed);
        Value::from_heap_ptr(ptr)
    }

    pub fn mark(&self) -> ArenaMark {
        ArenaMark::new(self.objects.len())
    }

    pub fn release(&mut self, mark: ArenaMark) {
        self.objects.truncate(mark.position());
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.objects.capacity()
    }

    /// Remove all objects without releasing the Vec buffer.
    pub fn clear(&mut self) {
        self.objects.clear();
    }
}

impl Default for FiberHeap {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static CURRENT_FIBER_HEAP: Cell<*mut FiberHeap> =
        const { Cell::new(std::ptr::null_mut()) };
}

/// Install a fiber heap as the current thread's active heap.
///
/// # Safety
/// Caller must ensure the FiberHeap outlives the installation.
pub unsafe fn install_fiber_heap(heap: *mut FiberHeap) {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(heap));
}

pub fn uninstall_fiber_heap() {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(std::ptr::null_mut()));
}

pub fn is_fiber_heap_installed() -> bool {
    CURRENT_FIBER_HEAP.with(|cell| !cell.get().is_null())
}

pub fn save_current_heap() -> *mut FiberHeap {
    CURRENT_FIBER_HEAP.with(|cell| cell.get())
}

/// Restore a previously saved heap pointer.
///
/// # Safety
/// Pointer must still be valid or null.
pub unsafe fn restore_saved_heap(saved: *mut FiberHeap) {
    CURRENT_FIBER_HEAP.with(|cell| cell.set(saved));
}

pub fn with_current_heap_mut<R>(f: impl FnOnce(&mut FiberHeap) -> R) -> Option<R> {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            None
        } else {
            Some(f(unsafe { &mut *ptr }))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::HeapObject;

    #[test]
    fn test_fiber_heap_alloc() {
        let mut heap = FiberHeap::new();
        let v = heap.alloc(HeapObject::String("hello".into()));
        assert_eq!(heap.len(), 1);
        assert!(v.is_heap());
        unsafe {
            let obj = crate::value::heap::deref(v);
            match obj {
                HeapObject::String(s) => assert_eq!(&**s, "hello"),
                _ => panic!("Expected String"),
            }
        }
    }

    #[test]
    fn test_fiber_heap_mark_release() {
        let mut heap = FiberHeap::new();
        let mark = heap.mark();
        heap.alloc(HeapObject::String("a".into()));
        heap.alloc(HeapObject::String("b".into()));
        heap.alloc(HeapObject::String("c".into()));
        assert_eq!(heap.len(), 3);
        heap.release(mark);
        assert_eq!(heap.len(), 0);
    }

    #[test]
    fn test_fiber_heap_nested_mark_release() {
        let mut heap = FiberHeap::new();
        let outer_mark = heap.mark();
        heap.alloc(HeapObject::String("outer".into()));
        let inner_mark = heap.mark();
        heap.alloc(HeapObject::String("inner".into()));
        assert_eq!(heap.len(), 2);
        heap.release(inner_mark);
        assert_eq!(heap.len(), 1);
        heap.release(outer_mark);
        assert_eq!(heap.len(), 0);
    }

    #[test]
    fn test_install_and_uninstall() {
        let mut heap = Box::new(FiberHeap::new());
        let ptr = &mut *heap as *mut FiberHeap;
        unsafe {
            install_fiber_heap(ptr);
        }
        assert!(is_fiber_heap_installed());
        assert!(with_current_heap_mut(|h| h.len()).is_some());
        uninstall_fiber_heap();
        assert!(!is_fiber_heap_installed());
    }

    #[test]
    fn test_no_heap_by_default() {
        // Ensure no heap is installed (may have been left by another test)
        uninstall_fiber_heap();
        assert!(!is_fiber_heap_installed());
        assert!(with_current_heap_mut(|h| h.len()).is_none());
    }

    #[test]
    fn test_save_restore() {
        let mut heap_a = Box::new(FiberHeap::new());
        let mut heap_b = Box::new(FiberHeap::new());
        heap_a.alloc(HeapObject::String("a".into()));
        heap_b.alloc(HeapObject::String("b1".into()));
        heap_b.alloc(HeapObject::String("b2".into()));

        let ptr_a = &mut *heap_a as *mut FiberHeap;
        let ptr_b = &mut *heap_b as *mut FiberHeap;

        unsafe {
            install_fiber_heap(ptr_a);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(1));

        let saved = save_current_heap();
        unsafe {
            install_fiber_heap(ptr_b);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(2));

        unsafe {
            restore_saved_heap(saved);
        }
        assert_eq!(with_current_heap_mut(|h| h.len()), Some(1));

        uninstall_fiber_heap();
    }
}
