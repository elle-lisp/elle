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

// ── Refcounting (no-ops — refcounting removed) ────────────────────

/// No-op: refcounting removed.
pub fn incref(_val: crate::value::Value) {}

/// No-op: refcounting removed.
pub fn decref(_val: crate::value::Value) {}

/// No-op: refcounting removed.
pub fn decref_and_free(_val: crate::value::Value) {}

/// Free all objects in a specific region on the current FiberHeap.
pub fn free_region(region_id: u16) {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).free_region(region_id) };
    }
}

/// Set the alloc_region on the current fiber heap.
///
/// Called by the VM dispatch loop before each allocating instruction
/// so that `alloc()` stamps every new object with this region.
#[inline]
pub fn set_alloc_region(region_id: u16) {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).alloc_region = region_id };
    }
}

/// Poison alloc_region to IDLE after an allocating instruction.
///
/// Any subsequent allocation before the next `set_alloc_region()` call
/// will panic — catching dispatch bugs and non-allocating instructions
/// that unexpectedly allocate.
#[inline]
pub fn poison_alloc_region() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).alloc_region = super::REGION_POISON_IDLE };
    }
}

/// Arm region tracking at dispatch-loop entry.
///
/// Sets `alloc_region = REGION_POISON_IDLE` so any allocation before
/// the first allocating instruction is caught.  Sets
/// `region_tracking_armed = true` so the ZERO poison is live.
#[inline]
pub fn arm_region_tracking() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe {
            (*ptr).region_tracking_armed = true;
            (*ptr).alloc_region = super::REGION_POISON_IDLE;
        }
    }
}

/// Reset alloc_region to 0 and disarm tracking on dispatch-loop exit.
///
/// Allocations that happen outside the dispatch loop (error
/// construction, compilation, fiber teardown) must not see IDLE
/// or trigger the ZERO panic.  The next `arm_region_tracking()`
/// call re-arms.
#[inline]
pub fn reset_alloc_region() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe {
            (*ptr).alloc_region = 0;
            (*ptr).region_tracking_armed = false;
        }
    }
}

/// Drop a single heap value: if owned by the current pool, run its
/// destructor, return the slab slot to the free list.
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
        // Run destructor if needed.
        if super::needs_drop((*obj_ptr).tag()) {
            std::ptr::drop_in_place(obj_ptr);
        }
        // Unlink from the allocation linked list (O(1)) and remove from
        // dtors (O(n) on dtors, which is typically short).
        heap.pool.unlink_alloc_ptr(obj_ptr);
        heap.pool.remove_from_dtors(obj_ptr);
        // Unlink from the region's singly-linked list so that a
        // subsequent free_region walk never visits this freed slot.
        let flat = heap.pool.slab.ptr_to_flat(obj_ptr);
        heap.pool.slab.unlink_from_region(flat);
        // Return slab slot to free list.
        heap.pool.dealloc_slot(obj_ptr);
        heap.pool.alloc_count = heap.pool.alloc_count.saturating_sub(1);
    }
}
