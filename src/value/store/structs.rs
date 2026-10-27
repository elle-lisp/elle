// audited: 2026-10-26
//! The `@struct` store: entries in key order in a B-tree of the container's region, read through a guard that answers lookups.
//!
//! docs/impl/mutable.md

use super::tree::{Node, Put, Tree, TreeRead};
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::types::TableKey;
use crate::value::Value;

/// The contents of an `@struct`: its entries in ascending key order, in a tree
/// whose nodes lie in one extent of the container's region. A stored key is
/// interned into the struct's own region before it is stored
/// (docs/impl/values.md).
pub struct StructStore(Tree<TableKey, Value>);

/// A read of an `@struct`'s entries. A write to the store panics while one
/// is live.
pub struct StructRead<'a>(TreeRead<'a, TableKey, Value>);

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
        self.0.get(key).is_some()
    }

    /// The entries in ascending key order.
    #[inline]
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&TableKey, &Value)> + ExactSizeIterator + '_ {
        self.0.iter()
    }

    /// The bytes of the extent the entries' nodes lie in.
    #[inline]
    pub fn node_bytes(&self) -> usize {
        self.0.node_bytes()
    }
}

impl StructStore {
    /// A store holding `entries`, whose keys ascend, are distinct and already
    /// live in the struct's region, in an extent of exactly the nodes they
    /// need, which `room` claims. The value it goes into counts their regions
    /// when it is allocated.
    pub(in crate::value) fn new(
        entries: &[(TableKey, Value)],
        room: impl FnOnce(usize) -> *mut Node<TableKey, Value>,
    ) -> Self {
        StructStore(Tree::new(entries.iter().copied(), room))
    }

    /// A store holding `entries`, as [`Self::new`] takes them, in an extent of
    /// `region` on `heap`. The container's shell must be born in `region` too.
    pub(in crate::value) fn new_in(
        heap: &mut FiberHeap,
        region: RuntimeRegion,
        entries: &[(TableKey, Value)],
    ) -> Self {
        StructStore::new(entries, |n| heap.alloc_room_in_region(n, region))
    }

    /// Read the entries. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> StructRead<'_> {
        StructRead(self.0.read())
    }

    /// Read the entries, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<StructRead<'_>> {
        self.0.try_read().map(StructRead)
    }

    /// Store `v` under `key`. When `key` is present the store keeps the key it
    /// holds, claims nothing, and answers the value `v` displaced. Otherwise it
    /// stores the key `vacant` hands back, which equals `key`, and answers that
    /// key. A put that needs nodes the store lacks calls the room `vacant`
    /// hands back with it for a larger extent, as `Extent::splice` does.
    pub(in crate::value) fn put<F>(
        &self,
        key: &TableKey,
        v: Value,
        vacant: impl FnOnce() -> (TableKey, F),
    ) -> Put<Value, TableKey>
    where
        F: FnOnce(usize) -> *mut Node<TableKey, Value>,
    {
        self.0.put(
            key,
            |held| std::mem::replace(held, v),
            || {
                let (stored, room) = vacant();
                ((stored, v), room)
            },
        )
    }

    /// Remove the entry under `key`, and answer the stored key with its
    /// value.
    pub(in crate::value) fn remove_entry(&self, key: &TableKey) -> Option<(TableKey, Value)> {
        self.0.take(key)
    }
}
