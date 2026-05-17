//! Runtime region: a pool (slab + bump) with a reference count.
//!
//! A region groups objects that share a lifetime. When the region's RC
//! reaches 0, all objects (and their inline data) are freed in bulk —
//! slab slots returned and bump arena rewound.
//!
//! ## Lifecycle
//!
//! 1. **Creation**: `FiberHeap::push_scope_mark()` (RegionEnter bytecode)
//!    records the current slab position.
//! 2. **Allocation**: objects allocated between RegionEnter/RegionExit
//!    belong to this region's scope.
//! 3. **Exit**: `FiberHeap::pop_scope_mark_and_release()` (RegionExit)
//!    frees all objects allocated since the mark.
//!
//! ## RC semantics (per-region, future)
//!
//! - RC counts durable references FROM OUTSIDE this region INTO it.
//! - `push arr val` where arr is in region A and val is in region B:
//!   `B.incref()`.
//! - Scope exit: if RC == 0, bulk free. If RC > 0, pin as orphan.
//!
//! ## Current status
//!
//! RuntimeRegion holds a pool with RC tracking. It is used by FiberHeap
//! for scope-mark-based deallocation. The per-region RC mechanism
//! (replacing per-slot RC) is not yet wired end-to-end.

use super::pool::SlabPool;

/// A runtime region: a pool with a reference count.
#[allow(dead_code)]
pub struct RuntimeRegion {
    /// Slab allocator + bump arena for this region's objects.
    pub pool: SlabPool,
    /// External reference count. Counts durable references from
    /// objects in OTHER regions into objects in THIS region.
    pub rc: u32,
}

#[allow(dead_code)]
impl RuntimeRegion {
    pub fn new() -> Self {
        RuntimeRegion {
            pool: SlabPool::new(),
            rc: 0,
        }
    }

    #[inline]
    pub fn incref(&mut self) {
        self.rc = self.rc.saturating_add(1);
    }

    #[inline]
    pub fn decref(&mut self) -> u32 {
        self.rc = self.rc.saturating_sub(1);
        self.rc
    }

    #[inline]
    pub fn is_pinned(&self) -> bool {
        self.rc > 0
    }

    pub fn alloc(&mut self, obj: crate::value::heap::HeapObject) -> crate::value::Value {
        self.pool.alloc(obj)
    }

    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
    ) -> crate::value::inline_slice::InlineSlice<T> {
        self.pool.alloc_inline_slice(items)
    }

    /// Bulk-free all objects in this region.
    pub fn release(mut self) {
        self.pool.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{HeapObject, Pair};
    use crate::value::Value;

    #[test]
    fn new_region_has_zero_rc() {
        let r = RuntimeRegion::new();
        assert_eq!(r.rc, 0);
        assert!(!r.is_pinned());
    }

    #[test]
    fn incref_decref() {
        let mut r = RuntimeRegion::new();
        r.incref();
        assert_eq!(r.rc, 1);
        assert!(r.is_pinned());
        let rc = r.decref();
        assert_eq!(rc, 0);
        assert!(!r.is_pinned());
    }

    #[test]
    fn alloc_and_release() {
        let mut r = RuntimeRegion::new();
        let _v = r
            .pool
            .alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(r.pool.live_count(), 1);
        r.release();
    }

    #[test]
    fn decref_saturates_at_zero() {
        let mut r = RuntimeRegion::new();
        let rc = r.decref();
        assert_eq!(rc, 0);
    }
}
