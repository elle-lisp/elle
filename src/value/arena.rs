//! Arena allocation layer.
//!
//! All allocations go through the current `FiberHeap` (root or child), reached
//! via the `CURRENT_FIBER_HEAP` thread-local in `fiber_heap/routing.rs`.
//!
//! - `alloc()` — allocate a heap object; lazily installs root heap if needed
//! - `alloc_permanent()` — Rc-backed allocation for objects that must outlive all scopes
//! - `deref()` — get a reference to a heap object from a Value
//! - `ArenaMark` — opaque position type for mark/release scope management
//! - `ArenaGuard` — RAII guard that releases the arena to a saved mark on drop
//! - `heap_arena_mark()` / `heap_arena_release()` — save/restore position on current FiberHeap

use std::rc::Rc;

use super::heap::HeapObject;
use super::Value;

/// Opaque mark for arena scope management.
///
/// Stores the FiberHeap alloc position and destructor list length at mark time.
pub struct ArenaMark {
    position: usize,
    dtor_len: usize,
    /// Length of the active custom allocator's `custom_ptrs` at mark time.
    /// Zero if no custom allocator is active.
    ///
    /// # Safety invariant
    ///
    /// This field records the position in the *current* (innermost) custom
    /// allocator's `custom_ptrs` at `RegionEnter` time. This is safe because
    /// `with-allocator` desugars to `defer`, which wraps the body in a fiber —
    /// the body's scope marks live on the child fiber's `FiberHeap`, separate
    /// from the parent's. If anyone calls `%install-allocator`/`%uninstall-allocator`
    /// directly without a fiber boundary between install and scope marks,
    /// `RegionExit` may dealloc from a popped allocator (use-after-free).
    /// **These primitives must only be used via the `with-allocator` macro.**
    custom_ptrs_len: usize,
    /// Depth of the scope bump stack at mark time. Used by `RegionExit`
    /// to verify that exactly one scope bump was pushed since this mark.
    bump_depth: usize,
}

impl ArenaMark {
    pub(crate) fn new_full(
        position: usize,
        dtor_len: usize,
        custom_ptrs_len: usize,
        bump_depth: usize,
    ) -> Self {
        ArenaMark {
            position,
            dtor_len,
            custom_ptrs_len,
            bump_depth,
        }
    }

    pub(crate) fn position(&self) -> usize {
        self.position
    }

    pub(crate) fn dtor_len(&self) -> usize {
        self.dtor_len
    }

    pub(crate) fn custom_ptrs_len(&self) -> usize {
        self.custom_ptrs_len
    }

    pub(crate) fn bump_depth(&self) -> usize {
        self.bump_depth
    }
}

/// RAII guard that releases the arena to a saved mark on drop.
pub struct ArenaGuard(Option<ArenaMark>);

impl Default for ArenaGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl ArenaGuard {
    pub fn new() -> Self {
        ArenaGuard(Some(heap_arena_mark()))
    }
}

impl Drop for ArenaGuard {
    fn drop(&mut self) {
        if let Some(mark) = self.0.take() {
            heap_arena_release(mark);
        }
    }
}

/// Save the current arena position on the current FiberHeap.
pub fn heap_arena_mark() -> ArenaMark {
    crate::value::fiber_heap::with_current_heap_mut(|heap| heap.mark()).unwrap_or_else(|| {
        // Lazy init: no heap installed (test context). Install root heap.
        let ptr = crate::value::fiber_heap::ensure_and_install_root_heap();
        unsafe { (*ptr).mark() }
    })
}

/// Release all arena allocations back to the mark, running destructors.
pub fn heap_arena_release(mark: ArenaMark) {
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let heap_ptr = if !heap_ptr.is_null() {
        heap_ptr
    } else {
        crate::value::fiber_heap::ensure_and_install_root_heap()
    };
    unsafe { (*heap_ptr).release(mark) };
}

/// Return the current arena position as an opaque checkpoint.
///
/// Retained for backward compatibility. In chunk 4, primitives
/// switch to using ArenaMark directly.
pub fn heap_arena_checkpoint() -> usize {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.len()).unwrap_or(0)
}

/// No-op stub — replaced by FiberHeap::release in chunk 4.
pub fn heap_arena_reset(_mark: usize) {
    // Intentional no-op: bumpalo does not support position-based
    // deallocation. This stub exists to keep compilation passing
    // until chunk 4 rewrites arena/checkpoint and arena/reset.
}

/// Current number of live objects in the thread-local (root) heap.
pub fn heap_arena_len() -> usize {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.len()).unwrap_or(0)
}

/// Current capacity (bumpalo chunk bytes) of the root heap.
pub fn heap_arena_capacity() -> usize {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.capacity()).unwrap_or(0)
}

/// Get the current object limit for the root heap.
pub fn heap_arena_object_limit() -> Option<usize> {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.object_limit()).flatten()
}

/// Set the object limit for the root heap. Returns the previous limit.
pub fn heap_arena_set_object_limit(limit: Option<usize>) -> Option<usize> {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.set_object_limit(limit)).flatten()
}

/// Get the peak object count for the root heap.
pub fn heap_arena_peak() -> usize {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.peak_alloc_count()).unwrap_or(0)
}

/// Reset peak to current count. Returns previous peak.
pub fn heap_arena_reset_peak() -> usize {
    crate::value::fiber_heap::with_current_heap_mut(|h| h.reset_peak()).unwrap_or(0)
}

/// Allocate a heap object and return a Value pointing to it.
///
/// Dispatches through the current FiberHeap. If no heap is installed
/// (test code running without a VM), ensures and installs the root
/// heap lazily before allocating.
pub fn alloc(obj: HeapObject) -> Value {
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    let heap_ptr = if !heap_ptr.is_null() {
        heap_ptr
    } else {
        // Lazy init: test code or pre-VM allocation. Install root heap.
        crate::value::fiber_heap::ensure_and_install_root_heap()
    };
    unsafe { (*heap_ptr).alloc(obj) }
}

/// Allocate a heap object permanently (bypasses arena tracking).
/// Used for objects that must outlive any mark/release scope (e.g., NativeFn).
pub fn alloc_permanent(obj: HeapObject) -> Value {
    let rc: Rc<HeapObject> = Rc::new(obj);
    let ptr = Rc::into_raw(rc) as *const ();
    Value::from_heap_ptr(ptr)
}

/// Get a reference to a heap object from a Value.
///
/// # Safety
/// The Value must be a heap pointer (is_heap() returns true).
#[inline]
pub unsafe fn deref(value: Value) -> &'static HeapObject {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    &*ptr
}

/// Clone (increment refcount) a heap value.
///
/// # Safety
/// The Value must be a heap pointer allocated via `alloc_permanent` (Rc-based).
#[inline]
pub unsafe fn clone_heap(value: Value) {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    let rc = Rc::from_raw(ptr);
    let _ = Rc::clone(&rc);
    std::mem::forget(rc); // Don't decrement refcount
}

/// Drop (decrement refcount) a heap value.
///
/// # Safety
/// The Value must be a heap pointer allocated via `alloc_permanent` (Rc-based).
#[inline]
pub unsafe fn drop_heap(value: Value) {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    drop(Rc::from_raw(ptr));
}
