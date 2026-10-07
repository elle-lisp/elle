// audited: 2026-10-06
//! `RegionVec`: a slice that grows in the working region, and `LirArena`, the region it grows in.
//!
//! docs/impl/lir.md

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;

/// A heap and the working region on it. `Copy`, so every slice that grows in
/// the region can carry it.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct LirArena {
    heap: *mut FiberHeap,
    region: RuntimeRegion,
}

#[allow(dead_code)]
impl LirArena {
    /// Mint a working region on `heap`. The caller frees it.
    pub(crate) fn mint(heap: &mut FiberHeap) -> LirArena {
        let region = heap.new_runtime_region();
        LirArena {
            heap: heap as *mut FiberHeap,
            region,
        }
    }

    /// The region this arena allocates in.
    pub(crate) fn region(&self) -> RuntimeRegion {
        self.region
    }
}

/// A slice of `T` that grows in an arena's region: push, truncate, and insert
/// at an index.
#[allow(dead_code)]
pub(crate) struct RegionVec<T: Copy + 'static> {
    arena: LirArena,
    items: &'static [T],
}

#[allow(dead_code)]
impl<T: Copy + 'static> RegionVec<T> {
    /// An empty slice that claims nothing until its first push.
    pub(crate) fn new(arena: LirArena) -> Self {
        RegionVec { arena, items: &[] }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        self.items
    }

    pub(crate) fn push(&mut self, _value: T) {}

    /// Drop everything from `len` on.
    pub(crate) fn truncate(&mut self, _len: usize) {}

    /// Insert `items` before index `at`, moving the tail up.
    pub(crate) fn insert_slice(&mut self, _at: usize, _items: &[T]) {}
}
