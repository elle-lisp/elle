//! Heap arena for temporary value allocation with mark/release semantics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::heap::HeapObject;
use super::Value;

// =============================================================================
// Heap Arena
//
// STOPGAP: This is a tactical fix for unbounded memory growth during macro
// expansion (see docs/heap-arena-plan.md). It is NOT a proper GC or lifetime
// system. Known unsoundnesses:
//
// 1. `deref()` returns `&'static HeapObject`. For arena-allocated objects,
//    this lifetime is a lie — the reference becomes dangling after release.
//    Safe only because no code path retains a `&HeapObject` across a release.
//
// 2. There is no type-level distinction between arena-allocated and
//    permanently-allocated Values. `clone_heap`/`drop_heap` assume Rc-backed
//    pointers; calling them on arena-allocated Values is undefined behavior.
//
// 3. `HeapObject::Drop` during `truncate` must not allocate Values (would
//    re-borrow the arena RefCell and panic). This constrains ExternalObject
//    Drop impls.
//
// When we move to a real memory management solution, delete this entire
// section and the `alloc_permanent` function.
// =============================================================================

struct HeapArena {
    /// Box provides pointer stability: HeapObject addresses survive Vec reallocation.
    #[allow(clippy::vec_box)]
    objects: Vec<Box<HeapObject>>,
    object_limit: Option<usize>,
    peak_object_count: usize,
}

impl HeapArena {
    fn new() -> Self {
        HeapArena {
            objects: Vec::new(),
            object_limit: None,
            peak_object_count: 0,
        }
    }
}

thread_local! {
    static HEAP_ARENA: RefCell<HeapArena> = RefCell::new(HeapArena::new());
}

thread_local! {
    static ALLOC_ERROR: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// Take the allocation error flag, clearing it. Returns `(count, limit)` if set.
pub fn take_alloc_error() -> Option<(usize, usize)> {
    ALLOC_ERROR.with(|e| e.take())
}

/// Opaque mark for arena scope management.
///
/// Stores the HEAP_ARENA Vec position (for the root fiber) and the
/// FiberHeap destructor list length (for child fibers with bumpalo).
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

/// Save the current arena position.
pub fn heap_arena_mark() -> ArenaMark {
    if let Some(mark) = crate::value::fiber_heap::with_current_heap_mut(|heap| heap.mark()) {
        return mark;
    }
    HEAP_ARENA.with(|arena| ArenaMark {
        position: arena.borrow().objects.len(),
        dtor_len: 0,
        custom_ptrs_len: 0,
        bump_depth: 0,
    })
}

/// Release all arena allocations back to the mark, dropping freed objects.
///
/// SAFETY CONSTRAINT: `HeapObject` variants dropped during truncate must not
/// allocate Values in their Drop impls. Doing so would re-borrow the arena
/// RefCell (already held by this truncate) and panic. This constrains
/// `ExternalObject` — plugin Drop impls must not call `Value::cons()` etc.
pub fn heap_arena_release(mark: ArenaMark) {
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    if !heap_ptr.is_null() {
        unsafe { (*heap_ptr).release(mark) };
        return;
    }
    HEAP_ARENA.with(|arena| arena.borrow_mut().objects.truncate(mark.position()))
}

/// Return the current root-fiber arena position as an opaque checkpoint.
///
/// Pass to [`heap_arena_reset`] to reclaim all objects allocated after
/// this point. Only meaningful for the global HEAP_ARENA (root fiber).
pub fn heap_arena_checkpoint() -> usize {
    HEAP_ARENA.with(|arena| arena.borrow().objects.len())
}

/// Truncate the root-fiber HEAP_ARENA back to `mark`, running Drop for
/// all objects allocated after that position.
///
/// # Safety contract (caller's responsibility)
///
/// Any `Value` pointing into the freed region is now dangling. The caller
/// must ensure those Values are unreachable before calling this.
///
/// # Panics
///
/// Does not panic if `mark > len` — silently no-ops (validated by caller).
pub fn heap_arena_reset(mark: usize) {
    HEAP_ARENA.with(|arena| {
        let mut a = arena.borrow_mut();
        if mark <= a.objects.len() {
            a.objects.truncate(mark);
        }
    })
}

/// Current number of live objects in the thread-local heap arena.
pub fn heap_arena_len() -> usize {
    if let Some(len) = crate::value::fiber_heap::with_current_heap_mut(|heap| heap.len()) {
        return len;
    }
    HEAP_ARENA.with(|arena| arena.borrow().objects.len())
}

/// Current capacity of the thread-local heap arena Vec.
pub fn heap_arena_capacity() -> usize {
    if let Some(cap) = crate::value::fiber_heap::with_current_heap_mut(|heap| heap.capacity()) {
        return cap;
    }
    HEAP_ARENA.with(|arena| arena.borrow().objects.capacity())
}

/// Get the current object limit for the global heap arena.
pub fn heap_arena_object_limit() -> Option<usize> {
    HEAP_ARENA.with(|a| a.borrow().object_limit)
}

/// Set the object limit for the global heap arena. Returns the previous limit.
pub fn heap_arena_set_object_limit(limit: Option<usize>) -> Option<usize> {
    HEAP_ARENA.with(|a| {
        let mut a = a.borrow_mut();
        let prev = a.object_limit;
        a.object_limit = limit;
        prev
    })
}

/// Get the peak object count for the global heap arena.
pub fn heap_arena_peak() -> usize {
    HEAP_ARENA.with(|a| a.borrow().peak_object_count)
}

/// Reset peak to current count. Returns previous peak.
pub fn heap_arena_reset_peak() -> usize {
    HEAP_ARENA.with(|a| {
        let mut a = a.borrow_mut();
        let prev = a.peak_object_count;
        a.peak_object_count = a.objects.len();
        prev
    })
}

/// Set the allocation error flag. Called by FiberHeap when its limit is exceeded.
pub fn set_alloc_error(count: usize, limit: usize) {
    ALLOC_ERROR.with(|e| e.set(Some((count, limit))));
}

/// Allocate a heap object on the thread-local arena and return a Value pointing to it.
///
/// Single thread-local read: check the raw pointer once, then dispatch.
/// `HeapObject` is not `Copy`, so we must not move it into a closure that
/// might not execute (that would silently drop the object).
pub fn alloc(obj: HeapObject) -> Value {
    let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
    if !heap_ptr.is_null() {
        return unsafe { (*heap_ptr).alloc(obj) };
    }
    HEAP_ARENA.with(|arena| {
        let mut a = arena.borrow_mut();
        if let Some(limit) = a.object_limit {
            let count = a.objects.len();
            if count >= limit {
                ALLOC_ERROR.with(|e| e.set(Some((count, limit))));
                return Value::NIL;
            }
        }
        let boxed = Box::new(obj);
        let ptr = &*boxed as *const HeapObject as *const ();
        a.objects.push(boxed);
        if a.objects.len() > a.peak_object_count {
            a.peak_object_count = a.objects.len();
        }
        Value::from_heap_ptr(ptr)
    })
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
