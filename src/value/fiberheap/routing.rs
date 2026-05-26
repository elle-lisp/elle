//! Thread-local fiber heap routing.

use super::FiberHeap;
use crate::hir::region::RegionId;
use std::cell::Cell;

thread_local! {
    static CURRENT_FIBER_HEAP: Cell<*mut FiberHeap> =
        const { Cell::new(std::ptr::null_mut()) };

    /// Current region for allocation routing.
    ///
    /// Set by the VM before NativeFn calls and macro expansion.
    /// Read by `arena::alloc()` to route allocations into the correct
    /// region's pages. 0 = no active region (panics on access).
    static CURRENT_ALLOC_REGION: Cell<RegionId> = const { Cell::new(0) };
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

/// Set the current allocation region for NativeFn calls.
///
/// The VM sets this before dispatching a NativeFn and clears it after.
/// NativeFn code calls `arena::alloc()` which samples this TLS variable
/// to route allocations into the correct region.
#[inline]
pub fn set_alloc_region(region_id: RegionId) {
    CURRENT_ALLOC_REGION.with(|cell| cell.set(region_id));
}

/// Get the current allocation region. Panics if no region is active.
#[inline]
pub fn get_alloc_region() -> RegionId {
    let id = CURRENT_ALLOC_REGION.with(|cell| cell.get());
    assert!(id != 0, "get_alloc_region: no active region");
    id
}

/// **DO NOT CALL THIS DIRECTLY.**
///
/// Raw read of the TLS alloc region. Returns 0 when no region is active.
/// The ONLY legitimate caller is the `with_alloc_region!` macro, which
/// needs to save the current value (possibly 0) before replacing it and
/// restore it afterward. Every other read must go through
/// `get_alloc_region()` which panics on 0 — catching misuse.
///
/// If you are reading this and thinking about calling it: don't.
/// Use `with_alloc_region!` or `with_transient_region!` instead.
#[inline]
#[allow(non_snake_case)]
pub fn read_alloc_region_FOR_USE_IN_with_alloc_region_ONLY() -> RegionId {
    CURRENT_ALLOC_REGION.with(|cell| cell.get())
}

/// Free all objects in a specific region on the current FiberHeap.
///
/// Panics if `region_id == 0`.
pub fn free_region(region_id: RegionId) {
    assert!(
        region_id != 0,
        "free_region called with region_id 0 — solver bug"
    );
    let ptr = current_heap_ptr();
    if !ptr.is_null() {
        let heap = unsafe { &mut *ptr };
        heap.free_region_physical(region_id);
    }
}
