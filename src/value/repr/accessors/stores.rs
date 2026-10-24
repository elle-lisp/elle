// audited: 2026-10-22
//! The accessors of the mutable containers: a read guard or a copy-out for each, and the stores themselves inside `value/`.
//!
//! docs/impl/mutable.md

use super::*;

use crate::value::heap::{deref, HeapObject};
use crate::value::store::{
    ArrayRead, ArrayStore, ByteRead, ByteStore, CellStore, SetRead, SetStore, StructRead,
    StructStore,
};

/// The heap object behind `v` when `v` carries `tag`.
#[inline]
fn object_tagged(v: &Value, tag: u64) -> Option<&'static HeapObject> {
    if v.tag == tag {
        // SAFETY: a value whose tag is a heap tag points at a heap object.
        Some(unsafe { deref(*v) })
    } else {
        None
    }
}

impl Value {
    /// Read an `@array`'s elements. `None` for every other value.
    #[inline]
    pub fn array_mut_read(&self) -> Option<ArrayRead<'_>> {
        self.array_store().map(ArrayStore::read)
    }

    /// Read an `@string`'s bytes. `None` for every other value, `@bytes`
    /// included.
    #[inline]
    pub fn string_mut_read(&self) -> Option<ByteRead<'_>> {
        self.string_store().map(ByteStore::read)
    }

    /// Read the bytes of `@bytes`. `None` for every other value, `@string`
    /// included.
    #[inline]
    pub fn bytes_mut_read(&self) -> Option<ByteRead<'_>> {
        self.bytes_store().map(ByteStore::read)
    }

    /// Read an `@struct`'s entries. `None` for every other value.
    #[inline]
    pub fn struct_mut_read(&self) -> Option<StructRead<'_>> {
        self.struct_store().map(StructStore::read)
    }

    /// Read an `@set`'s members. `None` for every other value.
    #[inline]
    pub fn set_mut_read(&self) -> Option<SetRead<'_>> {
        self.set_store().map(SetStore::read)
    }

    /// The elements of an array or an `@array`, copied. `None` for every other
    /// value.
    pub fn array_to_vec(&self) -> Option<Vec<Value>> {
        match self.array_mut_read() {
            Some(read) => Some(read.to_vec()),
            None => self.as_array().map(<[Value]>::to_vec),
        }
    }

    /// The entries of a struct or an `@struct` in key order, copied. `None`
    /// for every other value.
    pub fn struct_to_vec(&self) -> Option<Vec<(crate::value::types::TableKey, Value)>> {
        match self.struct_mut_read() {
            Some(read) => Some(read.iter().map(|(k, v)| (*k, *v)).collect()),
            None => self.as_struct().map(<[_]>::to_vec),
        }
    }

    /// The value in a user box.
    #[inline]
    pub fn lbox_get(&self) -> Option<Value> {
        self.lbox_store().map(CellStore::get)
    }

    /// The value in a compiler capture cell.
    #[inline]
    pub fn capture_cell_get(&self) -> Option<Value> {
        self.capture_cell_store().map(CellStore::get)
    }

    #[inline]
    pub(in crate::value) fn array_store(&self) -> Option<&'static ArrayStore> {
        match object_tagged(self, TAG_ARRAY_MUT)? {
            HeapObject::LArrayMut { data, .. } => Some(data),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn string_store(&self) -> Option<&'static ByteStore> {
        match object_tagged(self, TAG_STRING_MUT)? {
            HeapObject::LStringMut { data, .. } => Some(data),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn bytes_store(&self) -> Option<&'static ByteStore> {
        match object_tagged(self, TAG_BYTES_MUT)? {
            HeapObject::LBytesMut { data, .. } => Some(data),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn struct_store(&self) -> Option<&'static StructStore> {
        match object_tagged(self, TAG_STRUCT_MUT)? {
            HeapObject::LStructMut { data, .. } => Some(data),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn set_store(&self) -> Option<&'static SetStore> {
        match object_tagged(self, TAG_SET_MUT)? {
            HeapObject::LSetMut { data, .. } => Some(data),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn lbox_store(&self) -> Option<&'static CellStore> {
        match object_tagged(self, TAG_LBOX)? {
            HeapObject::LBox { cell, .. } => Some(cell),
            _ => None,
        }
    }

    #[inline]
    pub(in crate::value) fn capture_cell_store(&self) -> Option<&'static CellStore> {
        match object_tagged(self, TAG_CAPTURE_CELL)? {
            HeapObject::CaptureCell { cell, .. } => Some(cell),
            _ => None,
        }
    }
}
