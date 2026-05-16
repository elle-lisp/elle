//! Runtime region: a pool (slab + bump) with a reference count.
//!
//! A region groups objects that share a lifetime. When the region's RC
//! reaches 0, all objects (and their inline data) are freed in bulk —
//! slab slots returned and bump arena rewound. This replaces per-slot
//! reference counting with per-region RC.
//!
//! RC semantics:
//! - RC counts durable references FROM OUTSIDE this region INTO it.
//! - `push arr val` where arr is in region A and val is in region B:
//!   `B.incref()`.
//! - Scope exit: if RC == 0, bulk free. If RC > 0, pin as orphan.

use super::pool::SlabPool;
use crate::hir::region::RegionKind;

/// A runtime region: a pool with a reference count.
pub struct RuntimeRegion {
    /// Slab allocator + bump arena for this region's objects.
    pub pool: SlabPool,
    /// External reference count. Counts durable references from
    /// objects in OTHER regions into objects in THIS region.
    pub rc: u32,
    /// What kind of scope introduced this region.
    pub kind: RegionKind,
}

impl RuntimeRegion {
    /// Create a new empty region of the given kind.
    pub fn new(kind: RegionKind) -> Self {
        RuntimeRegion {
            pool: SlabPool::new(),
            rc: 0,
            kind,
        }
    }

    /// Increment the external reference count.
    #[inline]
    pub fn incref(&mut self) {
        self.rc = self.rc.saturating_add(1);
    }

    /// Decrement the external reference count. Returns the new RC.
    #[inline]
    pub fn decref(&mut self) -> u32 {
        self.rc = self.rc.saturating_sub(1);
        self.rc
    }

    /// Check if this region is pinned (has external references).
    #[inline]
    pub fn is_pinned(&self) -> bool {
        self.rc > 0
    }

    /// Bulk-free all objects in this region: run destructors, return
    /// slab slots, and rewind the bump arena. This is the O(1) scope
    /// exit path — no per-object RC check needed.
    pub fn release(mut self) {
        self.pool.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::region::RegionKind;
    use crate::value::heap::{HeapObject, Pair};
    use crate::value::Value;

    #[test]
    fn new_region_has_zero_rc() {
        let r = RuntimeRegion::new(RegionKind::Scope);
        assert_eq!(r.rc, 0);
        assert!(!r.is_pinned());
    }

    #[test]
    fn incref_decref() {
        let mut r = RuntimeRegion::new(RegionKind::Scope);
        r.incref();
        assert_eq!(r.rc, 1);
        assert!(r.is_pinned());
        let rc = r.decref();
        assert_eq!(rc, 0);
        assert!(!r.is_pinned());
    }

    #[test]
    fn alloc_and_release() {
        let mut r = RuntimeRegion::new(RegionKind::Scope);
        let _v = r.pool.alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
        assert_eq!(r.pool.live_count(), 1);
        r.release();
        // region is consumed; pool is torn down
    }

    #[test]
    fn decref_saturates_at_zero() {
        let mut r = RuntimeRegion::new(RegionKind::Scope);
        let rc = r.decref(); // already 0
        assert_eq!(rc, 0);
    }
}
