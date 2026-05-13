//! Thread-local fiber heap routing.

use super::FiberHeap;
use std::cell::Cell;

thread_local! {
    static CURRENT_FIBER_HEAP: Cell<*mut FiberHeap> =
        const { Cell::new(std::ptr::null_mut()) };
}

// Thread-local storage for the root fiber's persistent FiberHeap.
//
// Created once per thread on first access via `ensure_root_heap()`.
// Never freed (leaked via `Box::leak`) — lives for the thread's lifetime,
// so Values allocated on it remain valid after any individual VM is dropped.
//
// Stores a raw pointer to the leaked `FiberHeap`. Null until first
// `ensure_root_heap()` call.
thread_local! {
    static ROOT_HEAP: std::cell::Cell<*mut FiberHeap> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Ensure the thread-local root heap exists and return a pointer to it.
///
/// Creates the heap on first call (leaking it via `Box::leak`)
/// and stores the pointer. Subsequent calls return the same pointer.
///
/// The returned pointer is valid for the thread's lifetime.
pub fn ensure_root_heap() -> *mut FiberHeap {
    ROOT_HEAP.with(|cell| {
        let ptr = cell.get();
        if !ptr.is_null() {
            return ptr;
        }
        // Box::leak gives us a &'static mut FiberHeap. Cast to *mut for
        // storage in Cell<*mut>. The address is stable because Box heap-
        // allocates the value (and we never free it).
        let heap: &'static mut FiberHeap = Box::leak(Box::new(FiberHeap::new()));
        let ptr = heap as *mut FiberHeap;
        cell.set(ptr);
        ptr
    })
}

/// Install the root heap as the active fiber heap, replacing whatever
/// was active (typically null at VM startup, or a child heap if re-called).
///
/// Called by `VM::new()` to ensure the root fiber's FiberHeap is active
/// before any bytecode executes.
///
/// # Safety
/// The root heap pointer from `ensure_root_heap()` is valid for the
/// thread's lifetime.
pub fn install_root_heap() {
    let ptr = ensure_root_heap();
    // SAFETY: ptr is valid for thread lifetime (leaked Box).
    unsafe { install_fiber_heap(ptr) };
}

/// Ensure the root heap exists and is installed as the current heap.
///
/// Used by `alloc()` as a lazy fallback when called from test code
/// that runs without a `VM`. Returns the now-installed heap pointer.
///
/// In normal VM execution this is never called — the heap is installed
/// by `VM::new()` and remains installed for the VM's lifetime.
pub fn ensure_and_install_root_heap() -> *mut FiberHeap {
    let ptr = ensure_root_heap();
    // SAFETY: ptr is valid for thread lifetime (leaked Box).
    unsafe { install_fiber_heap(ptr) };
    ptr
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

/// Read the current fiber heap raw pointer (single TLS read).
/// Returns null if no heap is installed. Used by `heap::alloc()` to avoid
/// double TLS lookup (checking installed + dispatching are one operation).
pub fn current_heap_ptr() -> *mut FiberHeap {
    CURRENT_FIBER_HEAP.with(|cell| cell.get())
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

/// Check whether the current fiber heap has a shared allocator active.
pub fn current_heap_has_shared_alloc() -> bool {
    CURRENT_FIBER_HEAP.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            false
        } else {
            unsafe { (*ptr).has_shared_alloc() }
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

/// Enter outbox routing context on the current FiberHeap.
/// Allocations between outbox_enter and outbox_exit go to the outbox.
pub fn outbox_enter() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).outbox_enter() };
    }
}

/// Exit outbox routing context on the current FiberHeap.
pub fn outbox_exit() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).outbox_exit() };
    }
}

/// Push a scope mark on the current FiberHeap (called by VM `RegionEnter`).
pub fn region_enter() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).push_scope_mark() };
    }
}

/// Pop a scope mark and release scoped objects on the current FiberHeap
/// (called by VM `RegionExit`).
pub fn region_exit() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).pop_scope_mark_and_release() };
    }
}

/// Pop two scope marks and release only the range between them
/// (called by VM `RegionExitCall`).
pub fn region_exit_call() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).pop_call_scope_marks_and_release() };
    }
}

/// Rotate loop scope marks on the current FiberHeap (`RegionRotate`).
pub fn region_rotate() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).rotate_scope_marks() };
    }
}

// ── Refcounting ───────────────────────────────────────────────────

/// Increment the durable reference count for a heap value on the
/// current FiberHeap. No-op for non-heap values or if no heap installed.
pub fn incref(val: crate::value::Value) {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).incref_value(val) };
    }
}

/// Decrement the durable reference count for a heap value on the
/// current FiberHeap. No-op for non-heap values or if no heap installed.
pub fn decref(val: crate::value::Value) {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).decref_value(val) };
    }
}

/// Decrement the durable reference count for a heap value and free it
/// immediately if the refcount reaches 0. Called at mutation points
/// (put/push) when the old value is evicted from a collection.
pub fn decref_and_free(val: crate::value::Value) {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).decref_and_free(val) };
    }
}

/// Refcount-aware scope exit: pops scope mark and releases only
/// refcount-0 objects. Pinned objects (refcount > 0) survive.
pub fn region_exit_refcounted() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe {
            let heap = &mut *ptr;
            let mark = heap
                .scope_marks
                .pop()
                .expect("RegionExitRefcounted without matching RegionEnter");
            let dtors_before = heap.pool.dtors.len();
            heap.release_refcounted(mark);
            heap.scope_dtors_run += dtors_before - heap.pool.dtors.len();
        };
    }
}

/// Drop a single heap value: if owned by the current pool with refcount == 0,
/// run its destructor, return the slab slot to the free list, and mark it
/// as dropped in the bitmap. Does NOT walk children — values are freely
/// copyable and may be aliased by other live containers or closure envs.
/// Without local refcounting, recursive child freeing causes use-after-free.
pub fn drop_slot_value(val: crate::value::Value) {
    use crate::value::heap::HeapObject;

    let ptr = current_heap_ptr();
    if ptr.is_null() {
        return;
    }
    unsafe {
        let heap = &mut *ptr;
        let Some(heap_ptr) = val.as_heap_ptr() else {
            return;
        };
        if !heap.pool.slab_owns(heap_ptr) {
            return;
        }
        let obj_ptr = heap_ptr as *mut HeapObject;
        let rc = heap.pool.refcount(obj_ptr as *const _);
        if rc > 0 {
            if super::FiberHeap::trace_rc() {
                eprintln!(
                    "[trace:rc] drop_slot_value {:?} SKIPPED rc={}",
                    heap_ptr, rc
                );
            }
            return;
        }
        if super::FiberHeap::trace_rc() {
            eprintln!("[trace:rc] drop_slot_value {:?} FREED", heap_ptr);
        }
        #[cfg(debug_assertions)]
        {
            let tag = (*obj_ptr).tag();
            let flat = heap.pool.slab.ptr_to_flat(obj_ptr) as u32;
            eprintln!(
                "[drop_slot_value] FREEING flat {} ptr {:?} tag={:?} rc={}",
                flat, obj_ptr, tag, rc,
            );
        }
        // Decref all heap children (symmetric with incref in alloc).
        {
            let obj_ref = &*obj_ptr;
            let mut children = Vec::new();
            super::FiberHeap::collect_heap_children(obj_ref, &mut children);
            for child in children {
                heap.decref_value(child);
            }
        }
        // Run destructor if needed.
        if super::needs_drop((*obj_ptr).tag()) {
            std::ptr::drop_in_place(obj_ptr);
        }
        // Unlink from the allocation linked list (O(1)) and remove from
        // dtors (O(n) on dtors, which is typically short).
        heap.pool.unlink_alloc_ptr(obj_ptr);
        #[cfg(debug_assertions)]
        {
            let dtor_len_before = heap.pool.dtors.len();
            let scope_depth = heap.scope_marks.len();
            let dtor_indices: Vec<usize> = heap
                .pool
                .dtors
                .iter()
                .enumerate()
                .filter(|(_, &p)| p == obj_ptr)
                .map(|(i, _)| i)
                .collect();
            if !dtor_indices.is_empty() {
                // Log scope mark dtor_lens to see if any will be invalidated.
                let mark_dtor_lens: Vec<usize> =
                    heap.scope_marks.iter().map(|m| m.dtor_len()).collect();
                eprintln!(
                    "[drop_slot_value] {:?} in dtors at indices {:?}, \
                     dtors.len={}, scope_depth={}, mark_dtor_lens={:?}",
                    obj_ptr, dtor_indices, dtor_len_before, scope_depth, mark_dtor_lens
                );
            }
        }
        heap.pool.remove_from_dtors(obj_ptr);
        // Return slab slot to free list.
        heap.pool.dealloc_slot(obj_ptr);
        heap.pool.alloc_count = heap.pool.alloc_count.saturating_sub(1);
    }
}
