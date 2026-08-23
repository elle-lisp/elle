// Re-export everything the parent `regionstore` module's `use super::*;` brought
// into scope, so each themed submodule's `use super::*;` resolves the same names
// (RuntimeRegion, RegionStore, HeapObject, Value, …) it relied on before the split.
pub(crate) use super::*;
use crate::value::heap::Pair;

/// Wrap a raw id as a `RuntimeRegion` for tests (panics on 0).
pub(super) fn rr(n: u32) -> RuntimeRegion {
    RuntimeRegion::new(n).unwrap()
}

pub(super) fn cons_obj() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
}

mod edges;
mod forest;
mod generations;
mod recycle;
mod refcount;
