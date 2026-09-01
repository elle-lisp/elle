//! Region-and-heap-explicit value construction — the single source of
//! `HeapObject` construction shared by the `NativeCtx` capability
//! (`ctx.*`, which passes its own `heap`/`region`) and the compiler-internal
//! region-threaded population (which passes a region and the heap).
//!
//! Every fn here names BOTH its destination region (Rule 3: born in the right
//! region) and its heap, so the allocation target is visible in the signature:
//! identical `HeapObject`s and `alloc_in_region` calls, with the heap passed as
//! an explicit argument.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::hir::region::RuntimeRegion;
use crate::primitives::traitregistry::default_traits_for;
use crate::value::heap::{ExternalObject, HeapObject, HeapTag, Pair, TableKey};
use crate::value::FiberHeap;
use crate::value::Value;

/// Allocate a string (bytes inline in the arena) into `region` on `heap`.
#[inline]
pub(crate) fn string(heap: &mut FiberHeap, s: impl AsRef<str>, region: RuntimeRegion) -> Value {
    let slice = heap.alloc_region_slice_in_region::<u8>(s.as_ref().as_bytes(), region);
    let traits = default_traits_for(heap, HeapTag::LString);
    heap.alloc_in_region(HeapObject::LString { s: slice, traits }, region)
}

/// Allocate a cons cell into `region` on `heap`.
#[inline]
pub(crate) fn pair(heap: &mut FiberHeap, head: Value, tail: Value, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::Pair);
    heap.alloc_in_region(
        HeapObject::Pair(Pair {
            first: head,
            rest: tail,
            traits,
        }),
        region,
    )
}

/// Allocate an immutable array (elements inline in the arena) into `region`.
#[inline]
pub(crate) fn array(heap: &mut FiberHeap, elements: Vec<Value>, region: RuntimeRegion) -> Value {
    let slice = heap.alloc_region_slice_in_region::<Value>(&elements, region);
    let traits = default_traits_for(heap, HeapTag::LArray);
    heap.alloc_in_region(
        HeapObject::LArray {
            elements: slice,
            traits,
        },
        region,
    )
}

/// Allocate a mutable `@array` into `region` on `heap`.
#[inline]
pub(crate) fn array_mut(
    heap: &mut FiberHeap,
    elements: Vec<Value>,
    region: RuntimeRegion,
) -> Value {
    let traits = default_traits_for(heap, HeapTag::LArrayMut);
    heap.alloc_in_region(
        HeapObject::LArrayMut {
            data: Rc::new(RefCell::new(elements)),
            traits,
        },
        region,
    )
}

/// Allocate an empty mutable `@struct` into `region` on `heap`.
#[inline]
pub(crate) fn struct_mut(heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
    struct_mut_from(heap, BTreeMap::new(), region)
}

/// Allocate a mutable `@struct` with entries into `region` on `heap`.
#[inline]
pub(crate) fn struct_mut_from(
    heap: &mut FiberHeap,
    entries: BTreeMap<TableKey, Value>,
    region: RuntimeRegion,
) -> Value {
    let traits = default_traits_for(heap, HeapTag::LStructMut);
    heap.alloc_in_region(
        HeapObject::LStructMut {
            data: Rc::new(RefCell::new(entries)),
            traits,
        },
        region,
    )
}

/// Allocate an immutable struct (from an unsorted map) into `region` on `heap`.
#[inline]
pub(crate) fn struct_from(
    heap: &mut FiberHeap,
    fields: BTreeMap<TableKey, Value>,
    region: RuntimeRegion,
) -> Value {
    // BTreeMap iterates sorted, so the Vec is already sorted.
    let sorted: Vec<(TableKey, Value)> = fields.into_iter().collect();
    struct_from_sorted(heap, sorted, region)
}

/// Allocate an immutable struct (from pre-sorted entries) into `region`.
///
/// Keeps the Vec on the Rust heap because `TableKey::String` carries an owned
/// allocation; an arena memcpy would leak or double-free the String.
#[inline]
pub(crate) fn struct_from_sorted(
    heap: &mut FiberHeap,
    entries: Vec<(TableKey, Value)>,
    region: RuntimeRegion,
) -> Value {
    let traits = default_traits_for(heap, HeapTag::LStruct);
    heap.alloc_in_region(
        HeapObject::LStruct {
            data: entries,
            traits,
        },
        region,
    )
}

/// Allocate a closure into `region` on `heap`.
#[inline]
pub(crate) fn closure(
    heap: &mut FiberHeap,
    c: crate::value::heap::Closure,
    region: RuntimeRegion,
) -> Value {
    heap.alloc_in_region(
        HeapObject::Closure {
            closure: c,
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate a user box (`LBox`) into `region` on `heap`.
#[inline]
pub(crate) fn lbox(heap: &mut FiberHeap, value: Value, region: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::LBox {
            cell: Rc::new(RefCell::new(value)),
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate a compiler capture cell into `region` on `heap`.
#[inline]
pub(crate) fn capture_cell(heap: &mut FiberHeap, value: Value, region: RuntimeRegion) -> Value {
    heap.alloc_in_region(
        HeapObject::CaptureCell {
            cell: Rc::new(RefCell::new(value)),
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate a mutable `@string` into `region` on `heap`.
#[inline]
pub(crate) fn string_mut(heap: &mut FiberHeap, bytes: Vec<u8>, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::LStringMut);
    heap.alloc_in_region(
        HeapObject::LStringMut {
            data: Rc::new(RefCell::new(bytes)),
            traits,
        },
        region,
    )
}

/// Allocate immutable bytes (inline in the arena) into `region` on `heap`.
#[inline]
pub(crate) fn bytes(heap: &mut FiberHeap, data: Vec<u8>, region: RuntimeRegion) -> Value {
    let slice = heap.alloc_region_slice_in_region::<u8>(&data, region);
    let traits = default_traits_for(heap, HeapTag::LBytes);
    heap.alloc_in_region(
        HeapObject::LBytes {
            data: slice,
            traits,
        },
        region,
    )
}

/// Allocate mutable `@bytes` into `region` on `heap`.
#[inline]
pub(crate) fn bytes_mut(heap: &mut FiberHeap, data: Vec<u8>, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::LBytesMut);
    heap.alloc_in_region(
        HeapObject::LBytesMut {
            data: Rc::new(RefCell::new(data)),
            traits,
        },
        region,
    )
}

/// Allocate a syntax object into `region` on `heap`.
#[inline]
pub(crate) fn syntax(
    heap: &mut FiberHeap,
    s: crate::syntax::Syntax,
    region: RuntimeRegion,
) -> Value {
    heap.alloc_in_region(
        HeapObject::Syntax {
            syntax: Box::new(s),
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate an immutable set (sorted elements inline) into `region` on `heap`.
#[inline]
pub(crate) fn set(heap: &mut FiberHeap, items: BTreeSet<Value>, region: RuntimeRegion) -> Value {
    let sorted: Vec<Value> = items.into_iter().collect();
    let slice = heap.alloc_region_slice_in_region::<Value>(&sorted, region);
    let traits = default_traits_for(heap, HeapTag::LSet);
    heap.alloc_in_region(
        HeapObject::LSet {
            data: slice,
            traits,
        },
        region,
    )
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
            data: Rc::new(RefCell::new(items)),
            traits,
        },
        region,
    )
}

/// Allocate a managed FFI pointer into `region` on `heap` (NULL ⇒ nil).
#[inline]
pub(crate) fn managed_pointer(heap: &mut FiberHeap, addr: usize, region: RuntimeRegion) -> Value {
    if addr == 0 {
        return Value::NIL;
    }
    heap.alloc_in_region(
        HeapObject::ManagedPointer {
            addr: Cell::new(Some(addr)),
            traits: Value::NIL,
        },
        region,
    )
}

/// Allocate an external (plugin-provided) object into `region` on `heap`.
#[inline]
pub(crate) fn external<T: Any + 'static>(
    heap: &mut FiberHeap,
    type_name: &'static str,
    data: T,
    region: RuntimeRegion,
) -> Value {
    heap.alloc_in_region(
        HeapObject::External {
            obj: ExternalObject {
                type_name,
                data: Rc::new(data),
            },
            traits: Value::NIL,
        },
        region,
    )
}

/// Build a proper list (cons chain) into `region` on `heap` (Rule 3).
#[inline]
pub(crate) fn list(
    heap: &mut FiberHeap,
    values: impl IntoIterator<Item = Value>,
    region: RuntimeRegion,
) -> Value {
    values
        .into_iter()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .fold(Value::EMPTY_LIST, |acc, v| pair(heap, v, acc, region))
}

// ── Error values (structs `{:error :kind :message msg}`) ────────────

/// Construct an error value `{:error :kind :message msg}` born in `region` on
/// `heap` (Rule 3). The kind keyword is interned (immediate, no region).
#[inline]
pub(crate) fn error(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    region: RuntimeRegion,
) -> Value {
    error_extra(heap, kind, msg, &[], region)
}

/// Construct an error value with extra context fields, born in `region`.
#[inline]
pub(crate) fn error_extra(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    extra: &[(&str, Value)],
    region: RuntimeRegion,
) -> Value {
    let msg_val = string(heap, msg.into(), region);
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::keyword("error"), Value::keyword(kind));
    fields.insert(TableKey::keyword("message"), msg_val);
    for (key, val) in extra {
        fields.insert(TableKey::keyword(key), *val);
    }
    struct_from(heap, fields, region)
}

/// The runtime no-match error for `match`, born in `region` on `heap`.
#[inline]
pub(crate) fn match_fail(heap: &mut FiberHeap, val: Value, region: RuntimeRegion) -> Value {
    error_extra(
        heap,
        "match-error",
        format!("match: no pattern matched {} ({})", val, val.type_name()),
        &[("value", val)],
        region,
    )
}
