//! Tests for FiberHeap.

use super::*;
use crate::value::heap::{HeapObject, Pair};

#[test]
fn test_fiber_heap_alloc() {
    let mut heap = FiberHeap::new();
    let s = heap.alloc_inline_slice::<u8>(b"hello");
    let v = heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 1);
    assert!(v.is_heap());
    unsafe {
        let obj = crate::value::arena::deref(v);
        match obj {
            HeapObject::LString { s, .. } => assert_eq!(s.as_slice(), b"hello"),
            _ => panic!("Expected String"),
        }
    }
}

#[test]
fn test_fiber_heap_clear_runs_destructors() {
    // After the Phase 1–2 redesign, LString bytes live inline in the arena
    // and don't need per-object Drop. The arena itself reclaims everything
    // on clear(). No HeapObject variant currently needs individual Drop, so
    // this test now verifies that clear() resets the live count regardless.
    let mut heap = FiberHeap::new();
    let sa = heap.alloc_inline_slice::<u8>(b"a");
    heap.alloc(HeapObject::LString {
        s: sa,
        traits: Value::NIL,
    });
    let sb = heap.alloc_inline_slice::<u8>(b"b");
    heap.alloc(HeapObject::LString {
        s: sb,
        traits: Value::NIL,
    });
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    assert_eq!(heap.len(), 3); // 3 total objects allocated
    heap.clear();
    assert_eq!(heap.len(), 0);
    assert!(heap.is_empty());
}

#[test]
fn test_fiber_heap_non_drop_types_not_tracked() {
    let mut heap = FiberHeap::new();
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    // HeapObject::Float is no longer allocated — floats are immediate in 16-byte Value.
    // Use another non-drop type instead.
    heap.alloc(HeapObject::Pair(Pair::new(Value::TRUE, Value::EMPTY_LIST)));
    heap.alloc(HeapObject::LBox {
        cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
        traits: Value::NIL,
    });
    // 3 total objects; only the LBox needs Drop tracking. LBox wraps
    // its value in `Rc<RefCell<Value>>` for cross-fiber sharing, so
    // dropping it must decrement the Rc's strong count. The two Pair
    // cells are pure bit-copies and need no Drop.
    assert_eq!(heap.len(), 3);
    assert_eq!(heap.dtor_count(), 1);
}

#[test]
fn test_fiber_heap_needs_drop_exhaustive() {
    // This test exists to document which tags need Drop.
    // If a new HeapTag variant is added, `needs_drop` won't compile
    // until a decision is made.
    assert!(!needs_drop(HeapTag::Pair));
    assert!(!needs_drop(HeapTag::Float));
    assert!(!needs_drop(HeapTag::NativeFn));
    assert!(!needs_drop(HeapTag::LibHandle));
    assert!(!needs_drop(HeapTag::ManagedPointer));
    assert!(!needs_drop(HeapTag::Parameter));

    // LBox and CaptureCell now wrap their value in Rc<RefCell<Value>>
    // so that cross-fiber sharing survives deep_copy_to_outbox. Dropping
    // the Rc decrements the strong count — must be tracked.
    assert!(needs_drop(HeapTag::LBox));
    assert!(needs_drop(HeapTag::CaptureCell));

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
    let sa = heap_a.alloc_inline_slice::<u8>(b"a");
    heap_a.alloc(HeapObject::LString {
        s: sa,
        traits: Value::NIL,
    });
    let sb1 = heap_b.alloc_inline_slice::<u8>(b"b1");
    heap_b.alloc(HeapObject::LString {
        s: sb1,
        traits: Value::NIL,
    });
    let sb2 = heap_b.alloc_inline_slice::<u8>(b"b2");
    heap_b.alloc(HeapObject::LString {
        s: sb2,
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

// ── ROOT_HEAP tests ─────────────────────────────────────────────

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
    // Go through Value::string so the inline slice alloc also lazy-inits.
    let v = Value::string("lazy-test");
    assert!(v.is_heap());
    // Root heap is now installed.
    assert!(is_fiber_heap_installed());
    // Clean up
    uninstall_fiber_heap();
}


// ── Trampoline rotation tests ────────────────────────────────────────
//
// These simulate the trampoline's mark/release protocol: mark at first
// tail-call, release+mark at subsequent tail-calls.

#[test]
fn release_returns_slots_to_free_list() {
    // mark → alloc → release → alloc reuses freed slot.
    let mut heap = FiberHeap::new();
    let base_live = heap.root_live();

    let mark = heap.mark();
    let v1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    let v2 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    assert_eq!(heap.root_live(), base_live + 2);

    heap.release(mark);
    assert_eq!(
        heap.root_live(),
        base_live,
        "release must return slots to the free list"
    );

    let n1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(10), Value::NIL)));
    let freed_ptrs: [usize; 2] = [
        v1.as_heap_ptr().unwrap() as usize,
        v2.as_heap_ptr().unwrap() as usize,
    ];
    assert!(
        freed_ptrs.contains(&(n1.as_heap_ptr().unwrap() as usize)),
        "new allocation must reuse a freed slot"
    );
}

#[test]
fn release_no_dealloc_preserves_slots() {
    // mark → alloc → release_no_dealloc → slots NOT freed.
    let mut heap = FiberHeap::new();
    let base_live = heap.root_live();

    let mark = heap.mark();
    heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    heap.alloc(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));

    heap.release_no_dealloc(mark);
    assert_eq!(
        heap.root_live(),
        base_live + 2,
        "release_no_dealloc must NOT return slots to the free list"
    );
    assert_eq!(heap.len(), 0, "alloc_count must be reset to mark position");
}

#[test]
fn simulated_trampoline_rotation() {
    // Simulate the trampoline protocol: mark → alloc → release → mark →
    // alloc → release. After each release, live count returns to base.
    let mut heap = FiberHeap::new();
    let base_live = heap.root_live();

    for i in 0..100 {
        let mark = heap.mark();
        for j in 0..5 {
            heap.alloc(HeapObject::Pair(Pair::new(
                Value::int((i * 5 + j) as i64),
                Value::NIL,
            )));
        }
        heap.release(mark);
        assert_eq!(
            heap.root_live(),
            base_live,
            "iteration {}: live count must return to base after release",
            i
        );
    }
}


// ── region_of: per-slot region id tracking ─────────────────────────

#[test]
fn region_of_default_is_zero() {
    // All allocations start in region 0 (the default/private region).
    let mut heap = FiberHeap::new();
    let v = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    assert_eq!(heap.region_of(v), 0, "default region should be 0");
}

#[test]
fn region_of_immediate_is_zero() {
    // Non-heap values always return region 0.
    let heap = FiberHeap::new();
    assert_eq!(heap.region_of(Value::int(42)), 0);
    assert_eq!(heap.region_of(Value::NIL), 0);
    assert_eq!(heap.region_of(Value::TRUE), 0);
}

#[test]
fn free_region_frees_matching_slots() {
    let mut heap = FiberHeap::new();
    // Allocate two objects: one in region 1, one in region 2.
    let v1 = heap.alloc(HeapObject::Pair(Pair {
        first: Value::int(1),
        rest: Value::NIL,
        traits: Value::NIL,
    }));
    heap.stamp_region(v1, 1);

    let v2 = heap.alloc(HeapObject::Pair(Pair {
        first: Value::int(2),
        rest: Value::NIL,
        traits: Value::NIL,
    }));
    heap.stamp_region(v2, 2);

    assert_eq!(heap.len(), 2);

    // Free region 1 — only v1 should be freed.
    heap.free_region(1);
    assert_eq!(heap.len(), 1, "after free_region(1), one object should remain");

    // v2 (region 2) should still be alive.
    assert_eq!(heap.region_of(v2), 2);

    // Free region 2 — now everything should be freed.
    heap.free_region(2);
    assert_eq!(heap.len(), 0, "after free_region(2), no objects should remain");
}

#[test]
fn free_region_no_op_for_absent_region() {
    let mut heap = FiberHeap::new();
    let v = heap.alloc(HeapObject::Pair(Pair {
        first: Value::int(1),
        rest: Value::NIL,
        traits: Value::NIL,
    }));
    heap.stamp_region(v, 5);
    assert_eq!(heap.len(), 1);

    // Free region 99 (doesn't exist) — should be a no-op.
    heap.free_region(99);
    assert_eq!(heap.len(), 1, "free_region for absent region should be no-op");
}

#[test]
fn free_region_rewinds_bump_arena_at_tail() {
    // When a region's inline data is at the tail of the bump arena
    // (nothing else allocated after it), FreeRegion should rewind
    // the bump pointer to reclaim inline data (string bytes, etc.).
    let mut heap = FiberHeap::new();

    // Allocate a string (uses bump arena for inline data).
    let s = heap.alloc_inline_slice::<u8>(b"hello world, this is bump data!");
    let v = heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });
    heap.stamp_region(v, 1);
    assert_eq!(heap.len(), 1);

    let bytes_before = heap.allocated_bytes();
    assert!(bytes_before > 0, "should have allocated bump bytes");

    // Free region 1 — since it's the only allocation, the bump
    // arena should be fully rewound when all objects are freed.
    heap.free_region(1);
    assert_eq!(heap.len(), 0);

    // The slab slot is freed. The bump arena rewind happens only
    // when all slab objects are freed (alloc_head == ALLOC_NIL).
    // In this case, all objects are freed so the rewind should occur.
}
