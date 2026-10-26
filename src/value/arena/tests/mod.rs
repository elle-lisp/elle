// audited: 2026-10-26
//! The arena's tests, one themed file each, reading the names an inline test module would.
//!
//! docs/impl/region/ownership.md

pub(crate) use super::*;
pub(crate) use crate::value::heap::{HeapObject, Pair};

/// An empty `@array` in a fresh region on `heap`, and that region. A bare
/// heap has no default traitset, so the array carries no `traits` edge.
pub(crate) fn array_in_fresh_region(heap: &mut FiberHeap) -> (Value, RuntimeRegion) {
    let r = heap.new_runtime_region();
    (crate::value::build::array_mut(heap, Vec::new(), r), r)
}

/// An empty `@set` in a fresh region on `heap`, and that region, as
/// [`array_in_fresh_region`] builds an `@array`.
pub(crate) fn set_in_fresh_region(heap: &mut FiberHeap) -> (Value, RuntimeRegion) {
    let r = heap.new_runtime_region();
    (
        crate::value::build::set_mut(heap, std::collections::BTreeSet::new(), r),
        r,
    )
}

/// An empty `@struct` in a fresh region on `heap`, and that region, as
/// [`array_in_fresh_region`] builds an `@array`.
pub(crate) fn struct_in_fresh_region(heap: &mut FiberHeap) -> (Value, RuntimeRegion) {
    let r = heap.new_runtime_region();
    (crate::value::build::struct_mut(heap, r), r)
}

mod deref;
mod edges;
mod keys;
mod lookups;
mod macroscope;
mod refcount;
mod root;
mod selfedge;
