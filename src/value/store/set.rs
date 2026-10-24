// audited: 2026-10-23
//! The `@set` store: distinct values in order in an extent of the container's region, read through a guard that answers membership.
//!
//! docs/impl/mutable.md

use super::extent::{no_room, Extent, ExtentRead};
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

/// The contents of an `@set`: its members in ascending order, the layout an
/// immutable set's slice uses. Every member is frozen before it is stored.
pub struct SetStore(Extent<Value>);

/// A read of an `@set`'s members. A write to the store panics while one is
/// live.
pub struct SetRead<'a>(ExtentRead<'a, Value>);

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
        self.0.binary_search(v).is_ok()
    }

    /// The members in ascending order.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator + '_ {
        self.0.iter()
    }

    /// The members in ascending order, as one slice.
    #[inline]
    pub fn as_slice(&self) -> &[Value] {
        &self.0
    }
}

impl SetStore {
    /// A store holding `members`, which are ascending and distinct, in an
    /// extent of exactly their length that `room` claims.
    pub(in crate::value) fn new(members: &[Value], room: impl FnOnce(usize) -> *mut Value) -> Self {
        debug_assert!(
            members.windows(2).all(|pair| pair[0] < pair[1]),
            "a set store is built from ascending, distinct members"
        );
        SetStore(Extent::new(members, room))
    }

    /// A store holding `members`, which are ascending and distinct, in an
    /// extent of `region` on `heap`. The container's shell must be born in
    /// `region` too.
    pub(in crate::value) fn new_in(
        heap: &mut FiberHeap,
        region: RuntimeRegion,
        members: &[Value],
    ) -> Self {
        SetStore::new(members, |n| heap.alloc_room_in_region::<Value>(n, region))
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
    /// and answer whether it was stored. A store that is full calls `room`
    /// for a larger extent, as `Extent::splice` does.
    pub(in crate::value) fn insert(
        &self,
        v: Value,
        room: impl FnOnce(usize) -> *mut Value,
    ) -> bool {
        let at = match self.0.read().binary_search(&v) {
            Ok(_) => return false,
            Err(at) => at,
        };
        self.0.splice(at..at, &[v], room, |_| ());
        true
    }

    /// Remove the member equal to `v`, and answer the member the store held.
    pub(in crate::value) fn take(&self, v: &Value) -> Option<Value> {
        let at = self.0.read().binary_search(v).ok()?;
        Some(
            self.0
                .splice(at..at + 1, &[], no_room, |removed| removed[0]),
        )
    }
}
