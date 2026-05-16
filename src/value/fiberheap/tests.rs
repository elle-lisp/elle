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

// ── Scope mark stack tests ────────────────────────────────────

#[test]
#[should_panic(expected = "RegionExit without matching RegionEnter")]
fn test_scope_mark_pop_empty_panics() {
    let mut heap = FiberHeap::new();
    heap.pop_scope_mark_and_release();
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

// ── Region slot recycling tests ──────────────────────────────────────
//
// These tests verify that RegionExit returns slab slots to the free list.
// They are #[ignore] until scope eligibility for while/loop is routed
// through region inference (follow-up branch).

#[test]
fn region_exit_returns_slots_to_free_list() {
    // RegionExit must return slab slots to the free list so subsequent
    // allocations reuse them. This is the Phase 1 enabling condition:
    // escape-analysis-gated scope reclamation can safely deallocate
    // because the analysis proves no values escape the scope.
    let mut heap = FiberHeap::new();

    // Allocate 3 objects outside any scope (these are "base" objects).
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let base_live = heap.root_live();
    assert_eq!(base_live, 3);

    // Enter a scope, allocate 4 objects, exit scope.
    heap.push_scope_mark();
    let v1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    let v2 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    let v3 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(3), Value::NIL)));
    let v4 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(4), Value::NIL)));
    assert_eq!(heap.root_live(), base_live + 4);

    // RegionExit runs dtors (none for Pair) and returns slab slots.
    heap.pop_scope_mark_and_release();
    assert_eq!(
        heap.root_live(),
        base_live,
        "RegionExit must return scoped slots to the free list"
    );

    // The scope-exit Values are now dangling — do not dereference them.
    // But new allocations should reuse those freed slots.
    let n1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(10), Value::NIL)));
    let n2 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(20), Value::NIL)));

    // Verify slot reuse: the new pointers should match the freed ones.
    // (The free list is LIFO, so we expect reverse order.)
    let freed_ptrs: [usize; 4] = [
        v1.as_heap_ptr().unwrap() as usize,
        v2.as_heap_ptr().unwrap() as usize,
        v3.as_heap_ptr().unwrap() as usize,
        v4.as_heap_ptr().unwrap() as usize,
    ];
    let new_ptr1 = n1.as_heap_ptr().unwrap() as usize;
    let new_ptr2 = n2.as_heap_ptr().unwrap() as usize;
    assert!(
        freed_ptrs.contains(&new_ptr1),
        "new allocation must reuse a freed slot"
    );
    assert!(
        freed_ptrs.contains(&new_ptr2),
        "new allocation must reuse a freed slot"
    );

    assert_eq!(heap.root_live(), base_live + 2);
}

#[test]
fn region_exit_reclaims_dtor_objects() {
    // RegionExit must run destructors AND return slots for objects that
    // need Drop (LString, Closure, etc.). Verifies that dtor ordering
    // is correct (dtors run before slot dealloc).
    let mut heap = FiberHeap::new();

    let s = heap.alloc_inline_slice::<u8>(b"scoped-string");
    heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });
    assert_eq!(heap.dtor_count(), 1);

    heap.push_scope_mark();
    let s1 = heap.alloc_inline_slice::<u8>(b"a");
    heap.alloc(HeapObject::LString {
        s: s1,
        traits: Value::NIL,
    });
    let s2 = heap.alloc_inline_slice::<u8>(b"b");
    heap.alloc(HeapObject::LString {
        s: s2,
        traits: Value::NIL,
    });
    assert_eq!(heap.dtor_count(), 3);
    let live_before = heap.root_live();

    heap.pop_scope_mark_and_release();

    assert_eq!(
        heap.dtor_count(),
        1,
        "RegionExit must run and truncate scoped dtors"
    );
    assert_eq!(
        heap.root_live(),
        live_before - 2,
        "RegionExit must return 2 scoped slots to the free list"
    );
}

#[test]
fn region_exit_call_returns_middle_range() {
    // RegionExitCall pops two marks and frees only the range between
    // them (arg temporaries). Objects before mark1 and after mark2
    // are preserved. Slots in the middle are returned to the free list.
    let mut heap = FiberHeap::new();

    // Pre-region objects
    heap.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
    let pre_live = heap.root_live();

    // mark1: region start
    heap.push_scope_mark();

    // Arg temporaries (these get freed)
    let t1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    let t2 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    let temp_live = heap.root_live();

    // mark2: barrier after args
    heap.push_scope_mark();

    // Callee's allocations (preserved)
    heap.alloc(HeapObject::Pair(Pair::new(Value::int(3), Value::NIL)));
    assert_eq!(heap.root_live(), temp_live + 1);

    heap.pop_call_scope_marks_and_release();

    // Only the 2 arg temporaries were freed
    assert_eq!(
        heap.root_live(),
        pre_live + 1,
        "RegionExitCall must free exactly the middle range"
    );

    // New allocation should reuse one of the freed temporary slots
    let n1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(99), Value::NIL)));
    let temp_ptrs: [usize; 2] = [
        t1.as_heap_ptr().unwrap() as usize,
        t2.as_heap_ptr().unwrap() as usize,
    ];
    assert!(
        temp_ptrs.contains(&(n1.as_heap_ptr().unwrap() as usize)),
        "new allocation must reuse a freed temporary slot"
    );
}

#[test]
fn region_exit_nested_scopes_dealloc_innermost_first() {
    // Nested RegionEnter/RegionExit must dealloc innermost scope's slots
    // first, then outer scope's. The free list is LIFO, so inner slots
    // are reused first.
    let mut heap = FiberHeap::new();

    heap.push_scope_mark();
    let inner1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    heap.push_scope_mark();
    let inner2 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(2), Value::NIL)));
    assert_eq!(heap.root_live(), 2);

    // Exit inner scope — only inner2's slot is freed
    heap.pop_scope_mark_and_release();
    assert_eq!(heap.root_live(), 1);

    // Exit outer scope — inner1's slot is freed
    heap.pop_scope_mark_and_release();
    assert_eq!(heap.root_live(), 0);

    // Both slots should be reused
    let n1 = heap.alloc(HeapObject::Pair(Pair::new(Value::int(10), Value::NIL)));
    let freed_ptrs: [usize; 2] = [
        inner1.as_heap_ptr().unwrap() as usize,
        inner2.as_heap_ptr().unwrap() as usize,
    ];
    assert!(
        freed_ptrs.contains(&(n1.as_heap_ptr().unwrap() as usize)),
        "new allocation must reuse a freed slot"
    );
}

// ── Phase A: FiberHeap has no dead region fields ──────────────────────

#[test]
fn fiberheap_new_has_no_region_overhead() {
    let heap = FiberHeap::new();
    assert_eq!(heap.len(), 0);
    assert_eq!(heap.scope_depth(), 0);
    assert!(!heap.has_shared_alloc());
}

// ── Refcount-aware release (release_refcounted) ─────────────────────

#[test]
fn release_refcounted_keeps_pinned_objects() {
    // Objects with refcount > 0 survive release_refcounted.
    // This is the mechanism that allows push/put-referenced objects
    // to survive scope exit.
    let mut heap = FiberHeap::new();

    heap.push_scope_mark();
    let v = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    // Pin the object (simulating a push/put incref)
    heap.incref_value(v);
    assert_eq!(heap.refcount_value(v), 1);

    // RegionExit uses release_refcounted — pinned objects survive
    heap.pop_scope_mark_and_release();

    // The pinned object's slot is still live
    assert_eq!(heap.root_live(), 1, "pinned object must survive scope exit");
    assert_eq!(heap.refcount_value(v), 1, "refcount must be preserved");
}

#[test]
fn release_refcounted_frees_unpinned_objects() {
    // Objects with refcount == 0 are freed by release_refcounted.
    let mut heap = FiberHeap::new();

    heap.push_scope_mark();
    let _v = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    // No incref — refcount stays at 0 (the alloc-time child incref
    // doesn't pin the parent object itself)

    heap.pop_scope_mark_and_release();
    assert_eq!(heap.root_live(), 0, "unpinned object must be freed");
}


// ── Rotate scope marks (loop iteration reclamation) ────────────────

#[test]
fn rotate_scope_marks_frees_stale_iteration() {
    // Double-buffered rotation: push two marks, rotate frees the older
    // iteration's unpinned objects. Pinned objects (rc > 0) survive.
    let mut heap = FiberHeap::new();

    // Push initial pair of marks (prev, curr)
    heap.push_scope_mark(); // prev
    let _v_prev = heap.alloc(HeapObject::Pair(Pair::new(Value::int(0), Value::NIL)));
    heap.push_scope_mark(); // curr

    // First iteration
    let _v_curr = heap.alloc(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    let live_before_rotate = heap.root_live();
    assert_eq!(live_before_rotate, 2);

    // Rotate: frees prev's unpinned objects, shifts curr to prev
    heap.rotate_scope_marks();

    // v_prev has rc=0 (no external refs), so it's freed
    // The exact count depends on alloc-time child incref behavior
    let live_after_rotate = heap.root_live();
    assert!(
        live_after_rotate < live_before_rotate,
        "rotation must free at least one object from previous iteration"
    );

    // Clean up remaining marks
    heap.pop_scope_mark_and_release(); // curr
    heap.pop_scope_mark_and_release(); // prev
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
