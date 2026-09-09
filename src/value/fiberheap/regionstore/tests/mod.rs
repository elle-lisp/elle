// audited: 2026-09-09
// Re-exports what the region store's own scope holds, plus the two fixtures
// every themed file below builds a region from.
//
// docs/impl/region/ownership.md

pub(crate) use super::*;
use crate::value::heap::Pair;

/// Wrap a raw id as a `RuntimeRegion` for tests (panics on 0).
pub(super) fn rr(n: u32) -> RuntimeRegion {
    RuntimeRegion::new(n).unwrap()
}

pub(super) fn cons_obj() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
}

mod adopt;
mod edges;
mod generations;
mod recycle;
mod refcount;
mod reparent;
mod rescue;
mod subtree;
