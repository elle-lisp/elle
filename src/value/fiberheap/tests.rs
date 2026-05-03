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

// ── Shared allocator ownership tests ──────────────────────────────

#[test]
fn test_shared_alloc_routing() {
    let mut heap = FiberHeap::new();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);

    // Allocate via FiberHeap — should route to shared
    let s = heap.alloc_inline_slice::<u8>(b"routed");
    heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });

    // Private pool should be untouched
    assert_eq!(heap.len(), 0);

    // Shared allocator should have the allocation
    let sa = unsafe { &*sa_ptr };
    assert_eq!(sa.len(), 1);
}

#[test]
fn test_private_alloc_when_no_shared() {
    let mut heap = FiberHeap::new();
    // shared_alloc is null by default
    assert!(heap.shared_alloc().is_null());

    let s = heap.alloc_inline_slice::<u8>(b"private");
    heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 1);
    assert_eq!(heap.dtor_count(), 1);
}

#[test]
fn test_drop_tears_down_owned_shared() {
    // Create a FiberHeap with a shared allocator containing allocations,
    // then drop it. If Drop doesn't teardown, we'd leak inner heap allocs.
    let mut heap = FiberHeap::new();
    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);
    let s = heap.alloc_inline_slice::<u8>(b"will-be-dropped");
    heap.alloc(HeapObject::LString {
        s,
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

    // Create 3 shared allocs, allocate strings into each
    let sa1 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa1);
    let s1 = heap.alloc_inline_slice::<u8>(b"sa1-val");
    heap.alloc(HeapObject::LString {
        s: s1,
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    let sa2 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa2);
    let s2 = heap.alloc_inline_slice::<u8>(b"sa2-val");
    heap.alloc(HeapObject::LString {
        s: s2,
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    let sa3 = heap.create_shared_allocator();
    heap.set_shared_alloc(sa3);
    let s3 = heap.alloc_inline_slice::<u8>(b"sa3-val");
    heap.alloc(HeapObject::LString {
        s: s3,
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    assert_eq!(heap.shared_count(), 3);
    assert_eq!(unsafe { &*sa1 }.len(), 1);
    assert_eq!(unsafe { &*sa2 }.len(), 1);
    assert_eq!(unsafe { &*sa3 }.len(), 1);

    heap.clear();
    assert_eq!(heap.shared_count(), 0);
    assert!(heap.shared_alloc().is_null());
}

#[test]
fn test_shared_alloc_survives_private_clear() {
    // Shared allocs are NOT affected by private pool operations.
    // Private alloc_count/dtors are separate from shared.
    let mut heap = FiberHeap::new();

    let sa_ptr = heap.create_shared_allocator();
    heap.set_shared_alloc(sa_ptr);
    let s_shared = heap.alloc_inline_slice::<u8>(b"in-shared");
    heap.alloc(HeapObject::LString {
        s: s_shared,
        traits: Value::NIL,
    });
    heap.clear_shared_alloc();

    // Allocate privately
    let s_private = heap.alloc_inline_slice::<u8>(b"in-private");
    heap.alloc(HeapObject::LString {
        s: s_private,
        traits: Value::NIL,
    });
    assert_eq!(heap.len(), 1); // private count
    assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared count

    // Mark/release on private pool does not touch shared
    let mark = heap.mark();
    let s_scoped = heap.alloc_inline_slice::<u8>(b"scoped");
    heap.alloc(HeapObject::LString {
        s: s_scoped,
        traits: Value::NIL,
    });
    heap.release(mark);
    assert_eq!(heap.len(), 1); // back to 1
    assert_eq!(unsafe { &*sa_ptr }.len(), 1); // shared unchanged
}

// ── Flip* instructions ─────────────────────────────────────────────

fn alloc_drop_tracked(heap: &mut FiberHeap) {
    // Allocate an LString — it's in `needs_drop=true` territory, so it
    // enters both `allocs` and `dtors` lists. This is exactly the kind
    // of allocation the rotation/flip path needs to free.
    let s = heap.alloc_inline_slice::<u8>(b"x");
    heap.alloc(HeapObject::LString {
        s,
        traits: Value::NIL,
    });
}

#[test]
fn flip_enter_and_exit_balance() {
    let mut heap = FiberHeap::new();
    assert_eq!(heap.flip_depth(), 0);
    heap.flip_enter();
    assert_eq!(heap.flip_depth(), 1);
    heap.flip_enter();
    assert_eq!(heap.flip_depth(), 2);
    heap.flip_exit();
    assert_eq!(heap.flip_depth(), 1);
    heap.flip_exit();
    assert_eq!(heap.flip_depth(), 0);
}

#[test]
fn flip_swap_resets_current_iteration_count() {
    // `FlipSwap` has the same semantics as the trampoline's implicit
    // rotation: current iteration's allocations move into the swap
    // pool, and `alloc_count` resets to the base mark. The previous
    // iteration (now swap) is reclaimed at the *next* swap/exit.
    let mut heap = FiberHeap::new();
    heap.flip_enter();

    alloc_drop_tracked(&mut heap);
    assert_eq!(heap.len(), 1, "current iteration has 1 live object");

    heap.flip_swap();
    assert_eq!(
        heap.len(),
        0,
        "after swap, current iteration's count is back at base"
    );

    alloc_drop_tracked(&mut heap);
    assert_eq!(heap.len(), 1);

    heap.flip_swap();
    assert_eq!(heap.len(), 0);

    heap.flip_exit();
    assert_eq!(heap.flip_depth(), 0);
}

#[test]
fn flip_exit_restores_caller_swap_pool() {
    // A nested flip frame must not touch the caller's swap generation.
    // After an inner enter/swap/exit, the outer's next swap continues
    // to see the generation it set up before nesting — i.e. the swap
    // pool pointer is restored, not overwritten.
    //
    // We observe this through `rotation_freed`: once the outer calls
    // `flip_swap` again after the inner returns, its own (pre-inner)
    // swap pool must be the one that gets torn down. If the inner
    // stomped the outer's swap_pool, the outer's next swap would have
    // nothing to free.
    let mut heap = FiberHeap::new();

    heap.flip_enter();
    alloc_drop_tracked(&mut heap); // outer iter 0
    heap.flip_swap(); // outer iter 0 → outer's swap pool

    let freed_before = heap.rotation_freed;

    // Inner frame does its own rotations; must not see or touch
    // outer's swap pool.
    heap.flip_enter();
    alloc_drop_tracked(&mut heap);
    heap.flip_swap();
    alloc_drop_tracked(&mut heap);
    heap.flip_exit();

    // Now outer does another swap. Its saved swap pool (containing
    // outer's iter 0) should be what gets freed.
    alloc_drop_tracked(&mut heap); // outer iter 1
    heap.flip_swap();

    assert!(
        heap.rotation_freed > freed_before,
        "outer's swap pool survived the inner frame \
         (rotation_freed did not advance: before={}, after={})",
        freed_before,
        heap.rotation_freed
    );

    heap.flip_exit();
    assert_eq!(heap.flip_depth(), 0);
}

#[test]
fn flip_noop_without_frame() {
    // Isolated FlipSwap or FlipExit (no matching FlipEnter) must be
    // safe no-ops — the bytecode could be malformed, or the function
    // could have been lowered without auto-insertion and we still
    // want the instructions to be callable.
    let mut heap = FiberHeap::new();
    heap.flip_swap();
    heap.flip_exit();
    assert_eq!(heap.flip_depth(), 0);
}

// ── Region slot recycling tests ──────────────────────────────────────
//
// These tests verify that RegionExit returns slab slots to the free list.
// They are #[ignore] until scope eligibility for while/loop is routed
// through region inference (follow-up branch).

#[test]
#[ignore = "dealloc_slot disabled until scope eligibility uses region inference"]
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
#[ignore = "dealloc_slot disabled until scope eligibility uses region inference"]
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
#[ignore = "dealloc_slot disabled until scope eligibility uses region inference"]
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
#[ignore = "dealloc_slot disabled until scope eligibility uses region inference"]
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
