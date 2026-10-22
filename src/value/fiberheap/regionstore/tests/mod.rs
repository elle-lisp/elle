// audited: 2026-10-22
// Re-exports what the region store's own scope holds, plus the fixtures the
// themed files below build their regions from.
//
// docs/impl/region/ownership.md

pub(crate) use super::*;
use crate::value::heap::Pair;
use crate::value::store::ArrayStore;

/// Wrap a raw id as a `RuntimeRegion` for tests (panics on 0).
pub(super) fn rr(n: u32) -> RuntimeRegion {
    RuntimeRegion::new(n).unwrap()
}

pub(super) fn cons_obj() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
}

/// A mutable array in `region` that holds `vals`, the way a container stores
/// what it was given: the allocation records an edge to each value's region.
pub(super) fn holder(store: &mut RegionStore, region: RuntimeRegion, vals: &[Value]) -> Value {
    store.alloc_obj(
        region,
        HeapObject::LArrayMut {
            data: ArrayStore::new(vals.to_vec()),
            traits: Value::NIL,
        },
    )
}

mod adopt;
mod counters;
mod edges;
mod forest;
mod generations;
mod recycle;
mod refcount;
mod reparent;
mod rescue;
