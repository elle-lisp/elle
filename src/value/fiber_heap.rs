//! Per-fiber heap ownership and thread-local current-heap routing.
//!
//! `FiberHeap` uses bumpalo for fast bump allocation. Destructor tracking
//! ensures that `HeapObject` variants with inner heap allocations (`Vec`, `Rc`,
//! `BTreeMap`, `Box<str>`, etc.) have their `Drop` impls called on release/clear.
//!
//! The bump itself is only fully reset on `clear()` (fiber death / reset).
//! Partial `release(mark)` runs destructors but does not reclaim bump memory
//! — bumpalo has no partial reset. The real memory savings come from Drop
//! freeing the inner allocations (which live on the global heap, not in the
//! bump).
//!
//! ## Active allocator pointer
//!
//! `FiberHeap` carries an `active_allocator: *const bumpalo::Bump` pointer
//! that tracks which bump allocator the current execution context should use.
//! In Package 4 this is plumbing only (always points to the fiber's root bump).
//! In Package 5, `RegionEnter`/`RegionExit` will push/pop scope bumps and
//! redirect this pointer.
//!
//! The pointer is saved/restored:
//! - On **Call/Return** via `execute_bytecode_saving_stack` (Rust call stack)
//! - On **Yield/Resume** via the `SuspendedFrame.active_allocator` field
//! - On **Fiber swap** implicitly (each fiber owns its own `FiberHeap`)

use std::cell::Cell;

use crate::value::heap::{ArenaMark, HeapObject, HeapTag};
use crate::value::Value;

pub struct FiberHeap {
    bump: bumpalo::Bump,
    /// Raw pointers to bump-allocated HeapObjects that need Drop.
    /// Ordered by allocation time (oldest first).
    dtors: Vec<*mut HeapObject>,
    /// Total number of objects allocated (including those not needing Drop).
    alloc_count: usize,
    /// Pointer to the bump allocator that new allocations should use.
    /// In Package 4 this is write-only plumbing (always points to `self.bump`
    /// once initialized). In Package 5, `RegionEnter`/`RegionExit` will push
    /// sub-bumps and redirect this pointer to implement scope-based allocation.
    ///
    /// Starts as null; set via `init_active_allocator()` after the FiberHeap
    /// is in its final Box location (pointer stability requires this).
    active_allocator: *const bumpalo::Bump,
}

impl FiberHeap {
    pub fn new() -> Self {
        FiberHeap {
            bump: bumpalo::Bump::new(),
            dtors: Vec::new(),
            alloc_count: 0,
            active_allocator: std::ptr::null(),
        }
    }

    /// Set `active_allocator` to point to this FiberHeap's own bump.
    ///
    /// Must be called after the FiberHeap is in its final Box location
    /// (the pointer targets `&self.bump`, which must not move). Called by
    /// `with_child_fiber` when installing the child's heap.
    pub fn init_active_allocator(&mut self) {
        self.active_allocator = &self.bump as *const bumpalo::Bump;
    }

    /// Current active allocator pointer. Returns null if not yet initialized.
    pub fn active_allocator(&self) -> *const bumpalo::Bump {
        self.active_allocator
    }

    pub fn alloc(&mut self, obj: HeapObject) -> Value {
        let needs_drop = needs_drop(obj.tag());
        let ptr: &mut HeapObject = self.bump.alloc(obj);
        let raw = ptr as *mut HeapObject;
        if needs_drop {
            self.dtors.push(raw);
        }
        self.alloc_count += 1;
        Value::from_heap_ptr(raw as *const ())
    }

    pub fn mark(&self) -> ArenaMark {
        ArenaMark::new_with_dtor_len(self.alloc_count, self.dtors.len())
    }

    /// Run destructors for objects allocated after the mark, then truncate
    /// the destructor list. Does NOT reset the bump (no partial reset).
    pub fn release(&mut self, mark: ArenaMark) {
        let dtor_len = mark.dtor_len();
        // Walk in reverse: newest first
        for i in (dtor_len..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
        self.dtors.truncate(dtor_len);
        self.alloc_count = mark.position();
    }

    /// Total number of objects allocated since last clear/release.
    pub fn len(&self) -> usize {
        self.alloc_count
    }

    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    pub fn capacity(&self) -> usize {
        self.bump.chunk_capacity()
    }

    /// Drop all tracked objects and reset the bump allocator.
    ///
    /// Resets `active_allocator` to point to the root bump (in case
    /// Package 5 scope bumps were stacked). The pointer remains valid
    /// because `Bump::reset()` doesn't move the Bump struct.
    pub fn clear(&mut self) {
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
            }
        }
        self.dtors.clear();
        self.alloc_count = 0;
        self.bump.reset();
        // Reset to root bump in case scope bumps were active.
        if !self.active_allocator.is_null() {
            self.active_allocator = &self.bump as *const bumpalo::Bump;
        }
    }
}

impl Drop for FiberHeap {
    fn drop(&mut self) {
        // Run destructors for all tracked objects before the bump deallocates.
        // Without this, inner heap allocations (Vec buffers, Rc refcounts,
        // BTreeMap nodes, etc.) would leak when the bump is dropped.
        for i in (0..self.dtors.len()).rev() {
            unsafe {
                std::ptr::drop_in_place(self.dtors[i]);
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
fn needs_drop(tag: HeapTag) -> bool {
    match tag {
        // Copy/scalar innards — no heap allocations
        HeapTag::Cons => false,
        HeapTag::Cell => false,
        HeapTag::Float => false,
        HeapTag::NativeFn => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::Binding => false,
        // Inner heap allocations (Box<str>, Vec, Rc, BTreeMap, Arc, Cif, etc.)
        HeapTag::String => true,
        HeapTag::Array => true,
        HeapTag::Table => true,
        HeapTag::Struct => true,
        HeapTag::Closure => true,
        HeapTag::Tuple => true,
        HeapTag::Buffer => true,
        HeapTag::Bytes => true,
        HeapTag::Blob => true,
        HeapTag::Syntax => true,
        HeapTag::Fiber => true,
        HeapTag::ThreadHandle => true,
        HeapTag::FFISignature => true,
        HeapTag::FFIType => true,
        HeapTag::External => true,
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

/// Save the current `active_allocator` pointer from the installed FiberHeap.
/// Returns null if no FiberHeap is installed (root fiber).
pub fn save_active_allocator() -> *const bumpalo::Bump {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            std::ptr::null()
        } else {
            unsafe { (*ptr).active_allocator }
        }
    })
}

/// Restore a previously saved `active_allocator` pointer on the installed FiberHeap.
/// No-op if no FiberHeap is installed (root fiber).
pub fn restore_active_allocator(saved: *const bumpalo::Bump) {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if !ptr.is_null() {
            unsafe {
                (*ptr).active_allocator = saved;
            }
        }
    })
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
    use crate::value::heap::{Cons, HeapObject};

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
    fn test_fiber_heap_clear_runs_destructors() {
        let mut heap = FiberHeap::new();
        heap.alloc(HeapObject::String("a".into()));
        heap.alloc(HeapObject::String("b".into()));
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        assert_eq!(heap.len(), 3); // 3 total objects allocated
        assert_eq!(heap.dtors.len(), 2); // 2 need Drop (Strings)
        heap.clear();
        assert_eq!(heap.len(), 0);
        assert!(heap.is_empty());
    }

    #[test]
    fn test_fiber_heap_non_drop_types_not_tracked() {
        let mut heap = FiberHeap::new();
        heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
        heap.alloc(HeapObject::Float(42.5));
        heap.alloc(HeapObject::Cell(std::cell::RefCell::new(Value::NIL), false));
        assert_eq!(heap.len(), 3); // 3 total objects
        assert_eq!(heap.dtors.len(), 0); // None need Drop tracking
    }

    #[test]
    fn test_fiber_heap_needs_drop_exhaustive() {
        // This test exists to document which tags need Drop.
        // If a new HeapTag variant is added, `needs_drop` won't compile
        // until a decision is made.
        assert!(!needs_drop(HeapTag::Cons));
        assert!(!needs_drop(HeapTag::Cell));
        assert!(!needs_drop(HeapTag::Float));
        assert!(!needs_drop(HeapTag::NativeFn));
        assert!(!needs_drop(HeapTag::LibHandle));
        assert!(!needs_drop(HeapTag::ManagedPointer));
        assert!(!needs_drop(HeapTag::Binding));

        assert!(needs_drop(HeapTag::String));
        assert!(needs_drop(HeapTag::Array));
        assert!(needs_drop(HeapTag::Table));
        assert!(needs_drop(HeapTag::Struct));
        assert!(needs_drop(HeapTag::Closure));
        assert!(needs_drop(HeapTag::Tuple));
        assert!(needs_drop(HeapTag::Buffer));
        assert!(needs_drop(HeapTag::Bytes));
        assert!(needs_drop(HeapTag::Blob));
        assert!(needs_drop(HeapTag::Syntax));
        assert!(needs_drop(HeapTag::Fiber));
        assert!(needs_drop(HeapTag::ThreadHandle));
        assert!(needs_drop(HeapTag::FFISignature));
        assert!(needs_drop(HeapTag::FFIType));
        assert!(needs_drop(HeapTag::External));
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

    #[test]
    fn test_active_allocator_starts_null() {
        let heap = FiberHeap::new();
        assert!(heap.active_allocator().is_null());
    }

    #[test]
    fn test_init_active_allocator_points_to_bump() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let ptr = heap.active_allocator();
        assert!(!ptr.is_null());
        // The pointer should target the bump field inside the same FiberHeap.
        let bump_addr = &heap.bump as *const bumpalo::Bump;
        assert_eq!(ptr, bump_addr);
    }

    #[test]
    fn test_active_allocator_survives_clear() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let ptr_before = heap.active_allocator();
        heap.alloc(HeapObject::String("x".into()));
        heap.clear();
        let ptr_after = heap.active_allocator();
        // Pointer should still be valid and point to the same bump.
        assert!(!ptr_after.is_null());
        assert_eq!(ptr_before, ptr_after);
    }

    #[test]
    fn test_save_restore_active_allocator() {
        let mut heap = Box::new(FiberHeap::new());
        heap.init_active_allocator();
        let heap_ptr = &mut *heap as *mut FiberHeap;

        unsafe { install_fiber_heap(heap_ptr) };

        let saved = save_active_allocator();
        assert!(!saved.is_null());

        // Simulate Package 5: active_allocator changes to something else
        restore_active_allocator(std::ptr::null());
        assert!(save_active_allocator().is_null());

        // Restore original
        restore_active_allocator(saved);
        assert_eq!(save_active_allocator(), saved);

        uninstall_fiber_heap();
    }

    #[test]
    fn test_save_active_allocator_no_heap_installed() {
        uninstall_fiber_heap();
        // No heap installed — returns null, no panic
        assert!(save_active_allocator().is_null());
    }

    #[test]
    fn test_restore_active_allocator_no_heap_installed() {
        uninstall_fiber_heap();
        // No heap installed — no-op, no panic
        let fake_ptr = 0x1234 as *const bumpalo::Bump;
        restore_active_allocator(fake_ptr);
        // Still no heap, so save returns null
        assert!(save_active_allocator().is_null());
    }
}
