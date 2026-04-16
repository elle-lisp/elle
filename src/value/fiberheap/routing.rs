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

/// Eagerly drop a heap value: run the destructor (freeing inner heap data),
/// overwrite the slab slot with a `Cons(NIL, NIL)` sentinel, and decrement
/// the current heap's allocation count.
///
/// The slab slot stays in the pool's allocs/dtors — rotation/teardown see
/// the sentinel (needs_drop=false for Cons) and skip the destructor, then
/// dealloc the slot normally. No pool tracking coordination needed.
///
/// Called by the VM's `DropValue` instruction.
pub fn drop_value(value: crate::value::Value) {
    if !value.is_heap() {
        return;
    }
    let ptr = value.as_heap_ptr().unwrap() as *mut super::HeapObject;
    unsafe {
        let tag = (*ptr).tag();
        if super::needs_drop(tag) {
            std::ptr::drop_in_place(ptr);
        }
        // Overwrite with Cons(NIL, NIL) sentinel — needs_drop=false,
        // so rotation/teardown skip the destructor for this slot.
        std::ptr::write(
            ptr,
            super::HeapObject::Cons(crate::value::heap::Cons::new(
                crate::value::Value::NIL,
                crate::value::Value::NIL,
            )),
        );
    }
    // Decrement the allocation count on the current heap so resource
    // measurement reflects the eager drop.
    let heap_ptr = current_heap_ptr();
    if !heap_ptr.is_null() {
        unsafe { (*heap_ptr).decrement_alloc_count() };
    }
}

/// Reuse a slab slot for a new Cons cell. Runs the old value's destructor,
/// writes new Cons in-place, returns new Value with the same pointer.
/// Cons is needs_drop=false, so old dtors entries become no-ops (safe).
/// No alloc_count change — slot was already counted.
pub fn reuse_slot_cons(
    old_value: crate::value::Value,
    head: crate::value::Value,
    tail: crate::value::Value,
) -> crate::value::Value {
    let ptr = old_value.as_heap_ptr().unwrap() as *mut super::HeapObject;
    unsafe {
        let tag = (*ptr).tag();
        if super::needs_drop(tag) {
            std::ptr::drop_in_place(ptr);
        }
        std::ptr::write(
            ptr,
            super::HeapObject::Cons(crate::value::heap::Cons::new(head, tail)),
        );
    }
    let value_tag = super::HeapObject::Cons(crate::value::heap::Cons::new(
        crate::value::Value::NIL,
        crate::value::Value::NIL,
    ))
    .value_tag();
    crate::value::Value::from_heap_ptr(ptr as *const (), value_tag)
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
