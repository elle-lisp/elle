//! Thread-local fiber heap routing.

use super::FiberHeap;
use std::cell::Cell;

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

/// Push a scope mark on the current FiberHeap (called by VM `RegionEnter`).
/// No-op if no FiberHeap is installed (root fiber).
pub fn region_enter() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).push_scope_mark() };
    }
}

/// Pop a scope mark and release scoped objects on the current FiberHeap
/// (called by VM `RegionExit`).
/// No-op if no FiberHeap is installed (root fiber).
pub fn region_exit() {
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        unsafe { (*ptr).pop_scope_mark_and_release() };
    }
}
