// audited: 2026-10-22
//! The mutable-container constructors: each builds a container's store in its region, then its shell.
//!
//! docs/impl/mutable.md

use std::collections::{BTreeMap, BTreeSet};

use crate::hir::region::RuntimeRegion;
use crate::primitives::traitregistry::default_traits_for;
use crate::value::heap::{HeapObject, HeapTag, TableKey};
use crate::value::store::{ArrayStore, ByteStore, CellStore, SetStore, StructStore};
use crate::value::FiberHeap;
use crate::value::Value;

/// Allocate a mutable `@array` into `region` on `heap`.
#[inline]
pub(crate) fn array_mut(
    heap: &mut FiberHeap,
    elements: Vec<Value>,
    region: RuntimeRegion,
) -> Value {
    let traits = default_traits_for(heap, HeapTag::LArrayMut);
    array_mut_with_traits(heap, &elements, traits, region)
}

/// Allocate a mutable `@array` carrying `traits`, and its elements, into
/// `region` on `heap`.
pub(crate) fn array_mut_with_traits(
    heap: &mut FiberHeap,
    elements: &[Value],
    traits: Value,
    region: RuntimeRegion,
) -> Value {
    let data = ArrayStore::new_in(heap, region, elements);
    heap.alloc_in_region(HeapObject::LArrayMut { data, traits }, region)
}

/// Allocate an empty mutable `@struct` into `region` on `heap`.
#[inline]
pub(crate) fn struct_mut(heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
    struct_mut_from(heap, BTreeMap::new(), region)
}

/// Allocate a mutable `@struct` with entries into `region` on `heap`.
///
/// The keys are interned into `region` like an immutable struct's, so an
/// `@struct` never holds a key borrowed from the caller's region.
#[inline]
pub(crate) fn struct_mut_from(
    heap: &mut FiberHeap,
    entries: BTreeMap<TableKey, Value>,
    region: RuntimeRegion,
) -> Value {
    let interned: BTreeMap<TableKey, Value> = entries
        .iter()
        .map(|(k, v)| (k.intern_into(heap, region), *v))
        .collect();
    let traits = default_traits_for(heap, HeapTag::LStructMut);
    heap.alloc_in_region(
        HeapObject::LStructMut {
            data: StructStore::new(interned),
            traits,
        },
        region,
    )
}

/// Allocate a user box (`LBox`) into `region` on `heap`.
#[inline]
pub(crate) fn lbox(heap: &mut FiberHeap, value: Value, region: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LBox {
            cell: CellStore::new(value),
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate a compiler capture cell into `region` on `heap`.
#[inline]
pub(crate) fn capture_cell(
    heap: &mut FiberHeap,
    value: Value,
    origin: crate::value::heap::CellOrigin,
    region: RuntimeRegion,
) -> Value {
    heap.alloc_in_region(
        HeapObject::CaptureCell {
            cell: CellStore::new(value),
            origin,
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate a mutable `@string` into `region` on `heap`.
#[inline]
pub(crate) fn string_mut(heap: &mut FiberHeap, bytes: Vec<u8>, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::LStringMut);
    string_mut_with_traits(heap, &bytes, traits, region)
}

/// Allocate a mutable `@string` carrying `traits`, and its bytes, into
/// `region` on `heap`.
pub(crate) fn string_mut_with_traits(
    heap: &mut FiberHeap,
    bytes: &[u8],
    traits: Value,
    region: RuntimeRegion,
) -> Value {
    let data = ByteStore::new_in(heap, region, bytes);
    heap.alloc_in_region(HeapObject::LStringMut { data, traits }, region)
}

/// Allocate mutable `@bytes` into `region` on `heap`.
#[inline]
pub(crate) fn bytes_mut(heap: &mut FiberHeap, data: Vec<u8>, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::LBytesMut);
    bytes_mut_with_traits(heap, &data, traits, region)
}

/// Allocate mutable `@bytes` carrying `traits`, and its bytes, into `region`
/// on `heap`.
pub(crate) fn bytes_mut_with_traits(
    heap: &mut FiberHeap,
    data: &[u8],
    traits: Value,
    region: RuntimeRegion,
) -> Value {
    let data = ByteStore::new_in(heap, region, data);
    heap.alloc_in_region(HeapObject::LBytesMut { data, traits }, region)
}

/// Allocate a mutable set into `region` on `heap`.
#[inline]
pub(crate) fn set_mut(
    heap: &mut FiberHeap,
    items: BTreeSet<Value>,
    region: RuntimeRegion,
) -> Value {
    let traits = default_traits_for(heap, HeapTag::LSetMut);
    heap.alloc_in_region(
        HeapObject::LSetMut {
            data: SetStore::new(items),
            traits,
        },
        region,
    )
}
