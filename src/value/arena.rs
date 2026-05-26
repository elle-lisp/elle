//! Arena allocation layer.
//!
//! All runtime allocations go through the current `FiberHeap` via the
//! `CURRENT_FIBER_HEAP` thread-local in `fiberheap/routing.rs`.
//!
//! Two allocation paths:
//!
//! - `alloc()` / `alloc_inline_slice()` — runtime allocations that
//!   require an active TLS region (panics if region is 0 or immortal).
//! - `alloc_permanent()` / `alloc_inline_slice_permanent()` — compile-time
//!   allocations into the immortal region (region 1), never freed.

use super::heap::HeapObject;
use super::Value;

/// Region ID reserved for compile-time / permanent allocations.
/// Never freed. `alloc()` panics if it sees this.
pub const IMMORTAL_REGION: crate::hir::region::RegionId = 1;

// ── Runtime allocation (requires active TLS region) ─────────────────

/// Allocate a heap object into the current TLS region.
///
/// Panics if no region is active (0) or if the immortal region (1) is set.
pub fn alloc(obj: HeapObject) -> Value {
    let heap_ptr = ensure_heap();
    let region_id = crate::value::fiberheap::get_alloc_region();
    assert!(region_id != 0, "alloc(): no active region");
    assert!(
        region_id != IMMORTAL_REGION,
        "alloc(): immortal region is not for runtime use"
    );
    unsafe { (*heap_ptr).alloc_in_region(obj, region_id) }
}

/// Allocate an inline slice into the current TLS region.
///
/// Panics if no region is active (0) or if the immortal region (1) is set.
pub fn alloc_inline_slice<T: Copy + 'static>(items: &[T]) -> super::inline_slice::InlineSlice<T> {
    let heap_ptr = ensure_heap();
    let region_id = crate::value::fiberheap::get_alloc_region();
    assert!(region_id != 0, "alloc_inline_slice(): no active region");
    assert!(
        region_id != IMMORTAL_REGION,
        "alloc_inline_slice(): immortal region is not for runtime use"
    );
    unsafe { (*heap_ptr).alloc_inline_slice_in_region(items, region_id) }
}

// ── Permanent allocation (immortal region) ──────────────────────────

/// Allocate a heap object into the immortal region (region 1).
pub fn alloc_permanent(obj: HeapObject) -> Value {
    let heap_ptr = ensure_heap();
    unsafe { (*heap_ptr).alloc_in_region(obj, IMMORTAL_REGION) }
}

/// Allocate an inline slice into the immortal region (region 1).
pub fn alloc_inline_slice_permanent<T: Copy + 'static>(
    items: &[T],
) -> super::inline_slice::InlineSlice<T> {
    let heap_ptr = ensure_heap();
    unsafe { (*heap_ptr).alloc_inline_slice_in_region(items, IMMORTAL_REGION) }
}

/// No-op — immortal region values live forever.
/// # Safety
/// Always safe (no-op). Kept for API compatibility.
#[inline]
pub unsafe fn drop_heap(_value: Value) {}

// ── Runtime RC tracking (NativeFn use only, via TLS) ────────────────

/// Get the region ID for a heap value. Returns 0 for non-heap values.
pub fn region_of(val: Value) -> u16 {
    if !val.is_heap() {
        return 0;
    }
    let ptr = match val.as_heap_ptr() {
        Some(p) => p,
        None => return 0,
    };
    let heap_ptr = ensure_heap();
    let page_size = unsafe { (*heap_ptr).region_page_size() };
    unsafe { crate::value::fiberheap::regionpool::region_of_page_ptr(ptr, page_size) }
}

/// Increment a region's reference count.
pub fn incref_region(id: u16) {
    if id == 0 {
        return;
    }
    let heap_ptr = ensure_heap();
    unsafe {
        (*heap_ptr).incref_region(id);
    }
}

/// Decrement a region's reference count.
pub fn decref_region(id: u16) {
    if id == 0 {
        return;
    }
    let heap_ptr = ensure_heap();
    unsafe {
        (*heap_ptr).decref_region(id);
    }
}

/// Track a store into a mutable collection.
pub fn track_store(old: Value, new: Value) {
    let old_r = region_of(old);
    let new_r = region_of(new);
    if old_r == new_r {
        return;
    }
    incref_region(new_r);
    decref_region(old_r);
}

/// Track adding a value to a mutable collection.
pub fn track_insert(val: Value) {
    let r = region_of(val);
    if crate::config::get().has_trace("rc") && val.is_heap() {
        eprintln!(
            "[trace:rc] track_insert: val_type={} region={}",
            val.type_name(),
            r
        );
    }
    debug_assert!(
        !val.is_heap() || r != 0,
        "track_insert: heap value has region_of=0 — page header missing or corrupt"
    );
    incref_region(r);
}

/// Track removing a value from a mutable collection.
pub fn track_remove(val: Value) {
    let r = region_of(val);
    decref_region(r);
}

// ── Tracked mutation helpers ────────────────────────────────────────
//
// Combine mutable @array operations with region tracking.
// Both primitive and intrinsic paths must use these — never call
// as_array_mut().borrow_mut().push() directly without tracking.

/// Push a value into a mutable @array, tracking the cross-region reference.
/// Panics if `collection` is not an @array.
/// Returns the collection value.
pub fn tracked_push(collection: Value, elem: Value) -> Value {
    let vec_ref = collection
        .as_array_mut()
        .expect("tracked_push: expected @array");
    vec_ref.borrow_mut().push(elem);
    track_insert(elem);
    collection
}

/// Pop a value from a mutable @array, tracking the cross-region reference removal.
/// Panics if `collection` is not an @array or if it's empty.
/// Returns the popped value.
pub fn tracked_pop(collection: Value) -> Value {
    let vec_ref = collection
        .as_array_mut()
        .expect("tracked_pop: expected @array");
    let popped = vec_ref
        .borrow_mut()
        .pop()
        .expect("tracked_pop: empty @array");
    track_remove(popped);
    popped
}

/// Extend a mutable @array with multiple values, tracking each cross-region reference.
/// Panics if `collection` is not an @array.
/// Returns the collection value.
pub fn tracked_extend(collection: Value, elems: &[Value]) -> Value {
    let vec_ref = collection
        .as_array_mut()
        .expect("tracked_extend: expected @array");
    vec_ref.borrow_mut().extend_from_slice(elems);
    for &elem in elems {
        track_insert(elem);
    }
    collection
}

// ── Utility ─────────────────────────────────────────────────────────

/// Get a reference to a heap object from a Value.
///
/// # Safety
/// The Value must be a heap pointer (is_heap() returns true).
#[inline]
pub unsafe fn deref(value: Value) -> &'static HeapObject {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    &*ptr
}

/// Current number of live objects in the thread-local (root) heap.
pub fn heap_arena_len() -> usize {
    crate::value::fiberheap::with_current_heap_mut(|h| h.len()).unwrap_or(0)
}

// ── Test helper ─────────────────────────────────────────────────────

#[cfg(test)]
pub fn with_test_region<R>(f: impl FnOnce() -> R) -> R {
    let _ = ensure_heap();
    let rid = crate::lir::lower::fresh_region_id();
    crate::with_alloc_region!(rid => f())
}

/// Allocate a heap object into a fresh region, returning (value, region_id).
#[cfg(test)]
pub fn alloc_in_fresh_region(obj: super::heap::HeapObject) -> (Value, u16) {
    let heap_ptr = ensure_heap();
    let rid = crate::lir::lower::fresh_region_id();
    let val = unsafe { (*heap_ptr).alloc_in_region(obj, rid) };
    (val, rid)
}

/// Get the RC of a region via TLS heap.
#[cfg(test)]
pub fn region_rc(id: u16) -> u32 {
    crate::value::fiberheap::with_current_heap_mut(|h| h.region_rc(id)).unwrap_or(0)
}

/// Free a region via TLS heap.
#[cfg(test)]
pub fn free_region(id: u16) {
    let heap_ptr = ensure_heap();
    unsafe { (*heap_ptr).free_region_physical(id) };
}

// ── Internal ────────────────────────────────────────────────────────

fn ensure_heap() -> *mut crate::value::fiberheap::FiberHeap {
    let ptr = crate::value::fiberheap::current_heap_ptr();
    if !ptr.is_null() {
        ptr
    } else {
        crate::value::fiberheap::ensure_and_install_root_heap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{HeapObject, Pair};

    #[test]
    fn region_of_returns_zero_for_non_heap() {
        assert_eq!(region_of(Value::int(42)), 0);
        assert_eq!(region_of(Value::NIL), 0);
        assert_eq!(region_of(Value::TRUE), 0);
    }

    #[test]
    fn region_of_returns_correct_region() {
        let (val, rid) = alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert!(val.is_heap());
        assert_eq!(region_of(val), rid);
    }

    #[test]
    fn track_insert_increfs_region() {
        let (val, rid) = alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        let rc_before = region_rc(rid);
        track_insert(val);
        assert_eq!(region_rc(rid), rc_before + 1);
        track_remove(val);
    }

    #[test]
    fn track_remove_decrefs_region() {
        let (val, rid) = alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        track_insert(val);
        let rc_after_insert = region_rc(rid);
        track_remove(val);
        assert_eq!(region_rc(rid), rc_after_insert - 1);
    }

    #[test]
    fn track_store_same_region_is_noop() {
        let (val1, rid) =
            alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        let heap_ptr = ensure_heap();
        let val2 = unsafe {
            (*heap_ptr).alloc_in_region(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)), rid)
        };
        let rc_before = region_rc(rid);
        track_store(val1, val2);
        assert_eq!(
            region_rc(rid),
            rc_before,
            "same-region store should not change RC"
        );
    }

    #[test]
    fn track_insert_noop_for_immediates() {
        track_insert(Value::int(42));
        track_insert(Value::NIL);
    }

    #[test]
    fn mutable_array_push_keeps_region_alive() {
        let (val, rid_a) =
            alloc_in_fresh_region(HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));

        let (arr, rid_b) = alloc_in_fresh_region(HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
            traits: Value::NIL,
        });
        assert_ne!(rid_a, rid_b);

        let rc_before = region_rc(rid_a);
        crate::with_alloc_region!(rid_b => {
            let _ = crate::primitives::seq::seq_push(&arr, val);
        });
        assert_eq!(
            region_rc(rid_a),
            rc_before + 1,
            "push should incref val's region"
        );

        free_region(rid_a);

        crate::with_alloc_region!(rid_b => {
            let _ = crate::primitives::seq::seq_pop(&arr);
        });
        // rc was 2 (init=1 + push=1), free_region decrefs to 1,
        // pop decrefs to 0 → region fully freed.
        assert_eq!(
            region_rc(rid_a),
            0,
            "pop should decref val's region to 0 (freed)"
        );
    }
}
