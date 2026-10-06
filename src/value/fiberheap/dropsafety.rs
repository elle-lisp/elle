// audited: 2026-10-06
//! Per-tag drop/cascade classification: which variants need `Drop`, and which hold region references.
//!
//! docs/impl/region/model.md
//!
//! These two exhaustive predicates drive region teardown: which HeapObject
//! variants own inner allocations that must run Drop, and which non-dtor
//! variants still hold Value references that need cascade decref. Kept apart
//! from the `FiberHeap` machinery because they are pure tag → bool tables with
//! no wildcard arm — a new variant forces a decision here (compile error).

use crate::value::heap::HeapTag;

/// Exhaustive check: does this HeapObject variant have inner heap allocations
/// that require Drop? No wildcard arm — adding a new HeapObject variant
/// forces a decision here (compile error).
pub(crate) fn needs_drop(tag: HeapTag) -> bool {
    match tag {
        HeapTag::Pair => false,
        HeapTag::LBox => true,
        HeapTag::CaptureCell => true,
        HeapTag::Float => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::LString => true,
        HeapTag::LArrayMut => true,
        HeapTag::LStructMut => true,
        HeapTag::LStruct => true,
        HeapTag::Closure => true,
        HeapTag::LArray => true,
        HeapTag::LStringMut => true,
        HeapTag::LBytes => true,
        HeapTag::LBytesMut => true,
        HeapTag::Syntax => true,
        HeapTag::Fiber => true,
        HeapTag::ThreadHandle => true,
        HeapTag::FFISignature => true,
        HeapTag::FFIType => true,
        HeapTag::External => true,
        HeapTag::Parameter => false,
        HeapTag::LSet => true,
        HeapTag::LSetMut => true,
        // A header is one payload slice and owns no Rust heap memory
        // (docs/impl/region/template.md).
        HeapTag::ClosureTemplate => false,
    }
}

/// Does this non-dtor HeapObject variant hold Value references that
/// need cascade decref on region free?
pub(crate) fn holds_value_refs(tag: HeapTag) -> bool {
    match tag {
        HeapTag::Pair => true,
        HeapTag::Parameter => true,
        // The header→payload edge into its compile unit's code region, and
        // the payload's constants.
        HeapTag::ClosureTemplate => true,
        HeapTag::Float => false,
        HeapTag::LibHandle => false,
        HeapTag::ManagedPointer => false,
        HeapTag::LBox
        | HeapTag::CaptureCell
        | HeapTag::LString
        | HeapTag::LArrayMut
        | HeapTag::LStructMut
        | HeapTag::LStruct
        | HeapTag::Closure
        | HeapTag::LArray
        | HeapTag::LStringMut
        | HeapTag::LBytes
        | HeapTag::LBytesMut
        | HeapTag::Syntax
        | HeapTag::Fiber
        | HeapTag::ThreadHandle
        | HeapTag::FFISignature
        | HeapTag::FFIType
        | HeapTag::External
        | HeapTag::LSet
        | HeapTag::LSetMut => false,
    }
}
