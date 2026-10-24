// audited: 2026-10-22
//! The `@struct` store: entries in key order, read through a guard that answers lookups.
//!
//! docs/impl/mutable.md

use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::value::types::TableKey;
use crate::value::Value;

/// The contents of an `@struct`. A stored key is interned into the struct's
/// own region before it is stored (docs/impl/values.md).
pub struct StructStore(Rc<RefCell<BTreeMap<TableKey, Value>>>);

/// A read of an `@struct`'s entries. A write to the store panics while one
/// is live.
pub struct StructRead<'a>(Ref<'a, BTreeMap<TableKey, Value>>);

impl StructRead<'_> {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn get(&self, key: &TableKey) -> Option<Value> {
        self.0.get(key).copied()
    }

    #[inline]
    pub fn contains_key(&self, key: &TableKey) -> bool {
        self.0.contains_key(key)
    }

    /// The entries in ascending key order.
    #[inline]
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&TableKey, &Value)> + ExactSizeIterator + '_ {
        self.0.iter()
    }
}

impl StructStore {
    /// A store holding `entries`, whose keys already live in the struct's
    /// region. The value it goes into counts their regions when it is
    /// allocated.
    pub fn new(entries: BTreeMap<TableKey, Value>) -> Self {
        StructStore(Rc::new(RefCell::new(entries)))
    }

    /// Read the entries. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> StructRead<'_> {
        StructRead(self.0.borrow())
    }

    /// Read the entries, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<StructRead<'_>> {
        self.0.try_borrow().ok().map(StructRead)
    }

    /// Store `v` under `key`, and answer the value it displaced. A key
    /// already present keeps the stored copy, so `key` is stored only when it
    /// is new.
    pub(in crate::value) fn insert(&self, key: TableKey, v: Value) -> Option<Value> {
        self.0.borrow_mut().insert(key, v)
    }

    /// Remove the entry under `key`, and answer the stored key with its
    /// value.
    pub(in crate::value) fn remove_entry(&self, key: &TableKey) -> Option<(TableKey, Value)> {
        self.0.borrow_mut().remove_entry(key)
    }
}
