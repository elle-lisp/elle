//! Tests for FiberHeap.

use super::*;
use crate::value::heap::{Cons, HeapObject};

#[test]
fn test_fiber_heap_alloc() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let v = heap.alloc(HeapObject::LString {
        s: "hello".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 1);
    assert!(v.is_heap());
    unsafe {
        let obj = crate::value::arena::deref(v);
        match obj {
            HeapObject::LString { s, .. } => assert_eq!(&**s, "hello"),
            _ => panic!("Expected String"),
        }
    }
}

#[test]
fn test_fiber_heap_clear_runs_destructors() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    heap.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::LString {
        s: "b".into(),
        traits: Value::NIL,
    });
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
    heap.init_active_allocator();
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::Float(42.5));
    heap.alloc(HeapObject::LBox {
        cell: std::cell::RefCell::new(Value::NIL),
        is_local: false,
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 3); // 3 total objects
    assert_eq!(heap.dtors.len(), 0); // None need Drop tracking
}

#[test]
fn test_fiber_heap_needs_drop_exhaustive() {
    // This test exists to document which tags need Drop.
    // If a new HeapTag variant is added, `needs_drop` won't compile
    // until a decision is made.
    assert!(!needs_drop(HeapTag::Cons));
    assert!(!needs_drop(HeapTag::LBox));
    assert!(!needs_drop(HeapTag::Float));
    assert!(!needs_drop(HeapTag::NativeFn));
    assert!(!needs_drop(HeapTag::LibHandle));
    assert!(!needs_drop(HeapTag::ManagedPointer));
    assert!(!needs_drop(HeapTag::Binding));
    assert!(!needs_drop(HeapTag::Parameter));

    assert!(needs_drop(HeapTag::LString));
    assert!(needs_drop(HeapTag::LArrayMut));
    assert!(needs_drop(HeapTag::LStructMut));
    assert!(needs_drop(HeapTag::LStruct));
    assert!(needs_drop(HeapTag::Closure));
    assert!(needs_drop(HeapTag::LArray));
    assert!(needs_drop(HeapTag::LStringMut));
    assert!(needs_drop(HeapTag::LBytes));
    assert!(needs_drop(HeapTag::LBytesMut));
    assert!(needs_drop(HeapTag::Syntax));
    assert!(needs_drop(HeapTag::Fiber));
    assert!(needs_drop(HeapTag::ThreadHandle));
    assert!(needs_drop(HeapTag::FFISignature));
    assert!(needs_drop(HeapTag::FFIType));
    assert!(needs_drop(HeapTag::External));
    assert!(needs_drop(HeapTag::LSet));
    assert!(needs_drop(HeapTag::LSetMut));
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
    heap_a.init_active_allocator();
    heap_b.init_active_allocator();
    heap_a.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap_b.alloc(HeapObject::LString {
        s: "b1".into(),
        traits: Value::NIL,
    });
    heap_b.alloc(HeapObject::LString {
        s: "b2".into(),
        traits: Value::NIL,
    });

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
fn test_init_active_allocator_is_noop() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    // Still Slab after init (no scope bumps active).
    assert!(matches!(heap.active_allocator, ActiveAlloc::Slab));
}

#[test]
fn test_save_active_allocator_no_heap_installed() {
    uninstall_fiber_heap();
    // No heap installed — returns Slab (the safe default), no panic.
    assert!(matches!(save_active_allocator(), ActiveAlloc::Slab));
}

#[test]
fn test_restore_active_allocator_no_heap_installed() {
    uninstall_fiber_heap();
    // No heap installed — no-op, no panic.
    restore_active_allocator(ActiveAlloc::Slab);
    assert!(matches!(save_active_allocator(), ActiveAlloc::Slab));
}

// ── Scope mark stack tests ────────────────────────────────────

#[test]
#[should_panic(expected = "RegionExit without matching RegionEnter")]
fn test_scope_mark_pop_empty_panics() {
    let mut heap = FiberHeap::new();
    heap.pop_scope_mark_and_release();
}

// ── ROOT_HEAP tests (Chunk 1) ─────────────────────────────────────

#[test]
fn test_ensure_root_heap_idempotent() {
    // ensure_root_heap() must return the same pointer on every call.
    let p1 = ensure_root_heap();
    let p2 = ensure_root_heap();
    let p3 = ensure_root_heap();
    assert!(!p1.is_null());
    assert_eq!(p1, p2);
    assert_eq!(p2, p3);
}

#[test]
fn test_root_heap_active_allocator_initialized() {
    // After ensure_root_heap(), active_allocator must be Slab.
    let ptr = ensure_root_heap();
    let heap = unsafe { &*ptr };
    assert!(matches!(heap.active_allocator, ActiveAlloc::Slab));
}

#[test]
fn test_vm_new_installs_root_heap() {
    use crate::vm::core::VM;
    let _vm = VM::new();
    // After VM::new(), the current heap pointer must be non-null.
    assert!(is_fiber_heap_installed());
    // Clean up: uninstall so we don't interfere with subsequent tests.
    // (ROOT_HEAP thread-local persists, but CURRENT_FIBER_HEAP can be
    //  uninstalled for test isolation.)
    uninstall_fiber_heap();
}

// ── Chunk 3: lazy root heap init via alloc() ──────────────────────

#[test]
fn test_alloc_without_installed_heap_lazy_inits() {
    // alloc() with no heap installed triggers lazy root heap installation.
    uninstall_fiber_heap();
    // alloc() should not panic even with no heap installed.
    let v = crate::value::arena::alloc(HeapObject::LString {
        s: "lazy-test".into(),
        traits: Value::NIL,
    });
    assert!(v.is_heap());
    // Root heap is now installed.
    assert!(is_fiber_heap_installed());
    // Clean up
    uninstall_fiber_heap();
}

// ── Shared allocator ownership tests ──────────────────────────────

#[test]
fn test_shared_alloc_routing() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);

    // Allocate via FiberHeap — should route to shared
    heap.alloc(HeapObject::LString {
        s: "routed".into(),
        traits: Value::NIL,
    });

    // Private bump should be untouched
    assert_eq!(heap.alloc_count, 0);
    assert_eq!(heap.dtors.len(), 0);

    // Shared allocator should have the allocation
    let sa = unsafe { &*sa_ptr };
    assert_eq!(sa.len(), 1);
}

#[test]
fn test_private_alloc_when_no_shared() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    // shared_alloc is null by default
    assert!(heap.shared_alloc.is_null());

    heap.alloc(HeapObject::LString {
        s: "private".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.alloc_count, 1);
    assert_eq!(heap.dtors.len(), 1);
}

#[test]
fn test_drop_tears_down_owned_shared() {
    // Create a FiberHeap with a shared allocator containing allocations,
    // then drop it. If Drop doesn't teardown, we'd leak inner heap allocs.
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);
    heap.alloc(HeapObject::LString {
        s: "will-be-dropped".into(),
        traits: Value::NIL,
    });
    // Drop runs here — should not leak or panic.
    drop(heap);
}

// ── Shared allocator teardown lifecycle ────────────────────────────

#[test]
fn test_multiple_shared_allocs_all_torn_down() {
    // Create 3 shared allocators, allocate into each, verify clear()
    // tears down all three.
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();

    // Create 3 shared allocs, allocate strings into each
    let sa1 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa1);
    heap.alloc(HeapObject::LString {
        s: "sa1-val".into(),
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    let sa2 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa2);
    heap.alloc(HeapObject::LString {
        s: "sa2-val".into(),
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    let sa3 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa3);
    heap.alloc(HeapObject::LString {
        s: "sa3-val".into(),
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    assert_eq!(heap.owned_shared.len(), 3);
    assert_eq!(unsafe { &*sa1 }.len(), 1);
    assert_eq!(unsafe { &*sa2 }.len(), 1);
    assert_eq!(unsafe { &*sa3 }.len(), 1);

    heap.clear();
    assert!(heap.owned_shared.is_empty());
    assert!(heap.shared_alloc.is_null());
}

#[test]
fn test_shared_alloc_survives_private_clear() {
    // Shared allocs are NOT affected by private bump operations.
    // Private alloc_count/dtors are separate from shared.
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();

    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);
    heap.alloc(HeapObject::LString {
        s: "in-shared".into(),
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    // Allocate privately
    heap.alloc(HeapObject::LString {
        s: "in-private".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.alloc_count, 1); // private count
    assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared count

    // Mark/release on private bump does not touch shared
    let mark = heap.mark();
    heap.alloc(HeapObject::LString {
        s: "scoped".into(),
        traits: Value::NIL,
    });
    heap.release(mark);
    assert_eq!(heap.alloc_count, 1); // back to 1
    assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared unchanged
}
