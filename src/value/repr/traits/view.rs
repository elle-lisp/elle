// audited: 2026-11-18
//! One read of a kind's contents, whichever of its two forms the value has.
//!
//! docs/impl/mutable.md
//!
//! An immutable container holds a slice and its mutable twin holds a store
//! behind a read guard. Each function here hands a closure the contents of
//! either, so the order and the hash each state a kind's rule once. A member
//! and an entry are `Copy`, and each view hands them out by value, so two
//! views of two values yield one item type and compare as iterators.

use crate::value::heap::{deref, HeapObject};
use crate::value::types::TableKey;
use crate::value::Value;

/// The bytes of a string, an `@string`, bytes or `@bytes`.
pub(super) fn with_bytes<R>(v: &Value, f: impl FnOnce(&[u8]) -> R) -> R {
    // SAFETY: the caller holds a value of the string or bytes kind, which is
    // a heap value.
    match unsafe { deref(*v) } {
        HeapObject::LString { s, .. } => f(s.as_slice()),
        HeapObject::LBytes { data, .. } => f(data.as_slice()),
        HeapObject::LStringMut { data, .. } | HeapObject::LBytesMut { data, .. } => f(&data.read()),
        _ => unreachable!("with_bytes takes a string or bytes value"),
    }
}

/// The elements of an array or an `@array`.
pub(super) fn with_elements<R>(v: &Value, f: impl FnOnce(&[Value]) -> R) -> R {
    // SAFETY: the caller holds a value of the array kind, which is a heap
    // value.
    match unsafe { deref(*v) } {
        HeapObject::LArray { elements, .. } => f(elements.as_slice()),
        HeapObject::LArrayMut { data, .. } => f(&data.read()),
        _ => unreachable!("with_elements takes an array value"),
    }
}

/// The members of a set or an `@set`, in ascending order.
pub(super) fn with_members<R>(
    v: &Value,
    f: impl FnOnce(&mut dyn Iterator<Item = Value>) -> R,
) -> R {
    // SAFETY: the caller holds a value of the set kind, which is a heap value.
    match unsafe { deref(*v) } {
        HeapObject::LSet { data, .. } => f(&mut data.as_slice().iter().copied()),
        HeapObject::LSetMut { data, .. } => f(&mut data.read().iter().copied()),
        _ => unreachable!("with_members takes a set value"),
    }
}

/// The entries of a struct or an `@struct`, in ascending key order.
pub(super) fn with_entries<R>(
    v: &Value,
    f: impl FnOnce(&mut dyn Iterator<Item = (TableKey, Value)>) -> R,
) -> R {
    // SAFETY: the caller holds a value of the struct kind, which is a heap
    // value.
    match unsafe { deref(*v) } {
        HeapObject::LStruct { data, .. } => f(&mut data.as_slice().iter().copied()),
        HeapObject::LStructMut { data, .. } => f(&mut data.read().iter().map(|(k, v)| (*k, *v))),
        _ => unreachable!("with_entries takes a struct value"),
    }
}
