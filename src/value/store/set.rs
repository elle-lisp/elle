// audited: 2026-10-26
//! The `@set` store: distinct values in order in a B-tree of the container's region, read through a guard that answers membership.
//!
//! docs/impl/mutable.md

use super::tree::{Node, Tree, TreeRead};
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

/// The contents of an `@set`: its members in ascending order, in a tree whose
/// nodes lie in one extent of the container's region. A member is a key with
/// no value. Every member is frozen before it is stored.
pub struct SetStore(Tree<Value, ()>);

/// A read of an `@set`'s members. A write to the store panics while one is
/// live.
pub struct SetRead<'a>(TreeRead<'a, Value, ()>);

impl SetRead<'_> {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn contains(&self, v: &Value) -> bool {
        self.0.get(v).is_some()
    }

    /// The members in ascending order.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator + '_ {
        self.0.iter().map(|(member, _)| member)
    }

    /// The bytes of the extent the members' nodes lie in.
    #[inline]
    pub fn node_bytes(&self) -> usize {
        self.0.node_bytes()
    }
}

impl SetStore {
    /// A store holding `members`, which are ascending and distinct, in an
    /// extent of exactly the nodes they need, which `room` claims.
    pub(in crate::value) fn new(
        members: &[Value],
        room: impl FnOnce(usize) -> *mut Node<Value, ()>,
    ) -> Self {
        SetStore(Tree::new(members.iter().map(|&m| (m, ())), room))
    }

    /// A store holding `members`, which are ascending and distinct, in an
    /// extent of `region` on `heap`. The container's shell must be born in
    /// `region` too.
    pub(in crate::value) fn new_in(
        heap: &mut FiberHeap,
        region: RuntimeRegion,
        members: &[Value],
    ) -> Self {
        SetStore::new(members, |n| heap.alloc_room_in_region(n, region))
    }

    /// Read the members. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> SetRead<'_> {
        SetRead(self.0.read())
    }

    /// Read the members, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<SetRead<'_>> {
        self.0.try_read().map(SetRead)
    }

    /// Store `v` at its place in the order unless an equal member is present,
    /// and answer whether it was stored. An add that needs nodes the store
    /// lacks calls `room` for a larger extent, as `Extent::splice` does.
    pub(in crate::value) fn insert(
        &self,
        v: Value,
        room: impl FnOnce(usize) -> *mut Node<Value, ()>,
    ) -> bool {
        self.0.insert((v, ()), room)
    }

    /// Remove the member equal to `v`, and answer the member the store held.
    pub(in crate::value) fn take(&self, v: &Value) -> Option<Value> {
        self.0.take(v).map(|(member, ())| member)
    }
}
