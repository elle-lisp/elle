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
fn test_fiber_heap_mark_release() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let mark = heap.mark();
    heap.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::LString {
        s: "b".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::LString {
        s: "c".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 3);
    heap.release(mark);
    assert_eq!(heap.len(), 0);
}

#[test]
fn test_fiber_heap_nested_mark_release() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let outer_mark = heap.mark();
    heap.alloc(HeapObject::LString {
        s: "outer".into(),
        traits: Value::NIL,
    });
    let inner_mark = heap.mark();
    heap.alloc(HeapObject::LString {
        s: "inner".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 2);
    heap.release(inner_mark);
    assert_eq!(heap.len(), 1);
    heap.release(outer_mark);
    assert_eq!(heap.len(), 0);
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
fn test_clear_resets_scope_counters() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    // Simulate a scope region with an allocation
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "scoped".into(),
        traits: Value::NIL,
    });
    heap.pop_scope_mark_and_release();
    assert_eq!(heap.scope_enters(), 1);
    assert_eq!(heap.scope_dtors_run(), 1);
    // clear() must zero both counters
    heap.clear();
    assert_eq!(heap.scope_enters(), 0);
    assert_eq!(heap.scope_dtors_run(), 0);
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
    heap.alloc(HeapObject::LString {
        s: "x".into(),
        traits: Value::NIL,
    });
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

// ── Scope mark stack tests ────────────────────────────────────

#[test]
fn test_scope_mark_push_pop_lifecycle() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    heap.alloc(HeapObject::LString {
        s: "before".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 1);

    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "scoped".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 2);

    heap.pop_scope_mark_and_release();
    assert_eq!(heap.len(), 1); // back to pre-scope count
}

#[test]
fn test_scope_mark_nested() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));

    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "outer".into(),
        traits: Value::NIL,
    });

    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "inner".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 3);

    heap.pop_scope_mark_and_release(); // pops inner
    assert_eq!(heap.len(), 2);

    heap.pop_scope_mark_and_release(); // pops outer
    assert_eq!(heap.len(), 1); // only the cons cell
}

#[test]
fn test_scope_mark_runs_destructors() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    assert_eq!(heap.dtors.len(), 0);

    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::LString {
        s: "b".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    assert_eq!(heap.dtors.len(), 2); // 2 Strings need Drop

    heap.pop_scope_mark_and_release();
    assert_eq!(heap.dtors.len(), 0); // destructors ran, list truncated
    assert_eq!(heap.len(), 0);
}

#[test]
#[should_panic(expected = "RegionExit without matching RegionEnter")]
fn test_scope_mark_pop_empty_panics() {
    let mut heap = FiberHeap::new();
    heap.pop_scope_mark_and_release();
}

#[test]
fn test_scope_bump_reclaims_memory() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let bytes_before = heap.allocated_bytes();

    heap.push_scope_mark();
    // Allocate many objects in the scope bump
    for i in 0..100 {
        heap.alloc(HeapObject::LString {
            s: format!("obj-{}", i).into(),
            traits: Value::NIL,
        });
    }
    let bytes_during = heap.allocated_bytes();
    assert!(
        bytes_during > bytes_before,
        "scope allocations should increase bytes"
    );

    heap.pop_scope_mark_and_release();
    let bytes_after = heap.allocated_bytes();
    // After popping the scope bump, its memory is fully reclaimed.
    // bytes_after should equal bytes_before (root bump unchanged).
    assert_eq!(
        bytes_after, bytes_before,
        "scope bump memory should be fully reclaimed"
    );
}

#[test]
fn test_scope_bump_nested_reclaims_inner_only() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();

    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "outer".into(),
        traits: Value::NIL,
    });
    let bytes_after_outer = heap.allocated_bytes();

    heap.push_scope_mark();
    for i in 0..50 {
        heap.alloc(HeapObject::LString {
            s: format!("inner-{}", i).into(),
            traits: Value::NIL,
        });
    }
    let bytes_during_inner = heap.allocated_bytes();
    assert!(bytes_during_inner > bytes_after_outer);

    heap.pop_scope_mark_and_release(); // pops inner
    let bytes_after_inner_pop = heap.allocated_bytes();
    // Inner bump reclaimed, outer bump still alive
    assert_eq!(bytes_after_inner_pop, bytes_after_outer);

    heap.pop_scope_mark_and_release(); // pops outer
    let bytes_after_outer_pop = heap.allocated_bytes();
    // Both bumps reclaimed, back to root-only
    assert_eq!(bytes_after_outer_pop, 0);
}

#[test]
fn test_clear_clears_scope_marks() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "b".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.scope_marks.len(), 2);

    heap.clear();
    assert_eq!(heap.scope_marks.len(), 0);
    assert_eq!(heap.len(), 0);
}

#[test]
fn test_clear_clears_scope_bumps() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "a".into(),
        traits: Value::NIL,
    });
    heap.push_scope_mark();
    heap.alloc(HeapObject::LString {
        s: "b".into(),
        traits: Value::NIL,
    });
    assert_eq!(heap.scope_bumps.len(), 2);

    heap.clear();
    assert_eq!(heap.scope_bumps.len(), 0);
    assert_eq!(heap.allocated_bytes(), 0);
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
    // After ensure_root_heap(), active_allocator must be non-null.
    let ptr = ensure_root_heap();
    let heap = unsafe { &*ptr };
    assert!(!heap.active_allocator().is_null());
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

// ── Shared allocator ownership tests ──────────────────────────────

#[test]
fn test_create_shared_allocator() {
    let mut heap = FiberHeap::new();
    let ptr = heap.create_shared_allocator();
    assert!(!ptr.is_null());
    assert_eq!(heap.owned_shared.len(), 1);
}

#[test]
fn test_create_multiple_shared_allocators() {
    let mut heap = FiberHeap::new();
    let ptr1 = heap.create_shared_allocator();
    let ptr2 = heap.create_shared_allocator();
    assert!(!ptr1.is_null());
    assert!(!ptr2.is_null());
    assert_ne!(ptr1, ptr2);
    assert_eq!(heap.owned_shared.len(), 2);
}

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
fn test_clear_tears_down_owned_shared() {
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);

    heap.alloc(HeapObject::LString {
        s: "shared-val".into(),
        traits: Value::NIL,
    });
    assert_eq!(unsafe { &*sa_ptr }.len(), 1);

    heap.clear();
    assert!(heap.owned_shared.is_empty());
    assert!(heap.shared_alloc.is_null());
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
fn test_clear_tears_down_shared_alloc_dtors() {
    // Verify that clear() runs destructors in shared allocators.
    // String's inner Box<str> must be freed (dtors ran), verified
    // by checking the shared alloc's count goes to zero.
    let mut heap = FiberHeap::new();
    heap.init_active_allocator();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);

    heap.alloc(HeapObject::LString {
        s: "str-a".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::LString {
        s: "str-b".into(),
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
    {
        let sa = unsafe { &*sa_ptr };
        assert_eq!(sa.len(), 3);
    }

    // clear() should teardown the shared alloc (runs dtors, resets count)
    // then remove it from owned_shared.
    heap.clear();
    assert!(heap.owned_shared.is_empty());
    assert_eq!(heap.len(), 0);
}

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
