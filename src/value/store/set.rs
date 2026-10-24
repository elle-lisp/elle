// audited: 2026-10-22
//! The `@set` store: distinct values in order, read through a guard that answers membership.
//!
//! docs/impl/mutable.md

use std::cell::{Ref, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::value::Value;

/// The contents of an `@set`. Every member is frozen before it is stored.
pub struct SetStore(Rc<RefCell<BTreeSet<Value>>>);

/// A read of an `@set`'s members. A write to the store panics while one is
/// live.
pub struct SetRead<'a>(Ref<'a, BTreeSet<Value>>);

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
        self.0.contains(v)
    }

    /// The members in ascending order.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator + '_ {
        self.0.iter()
    }
}

impl SetStore {
    /// A store holding `members`. The value it goes into counts their
    /// regions when it is allocated.
    pub fn new(members: BTreeSet<Value>) -> Self {
        SetStore(Rc::new(RefCell::new(members)))
    }

    /// Read the members. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> SetRead<'_> {
        SetRead(self.0.borrow())
    }

    /// Read the members, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<SetRead<'_>> {
        self.0.try_borrow().ok().map(SetRead)
    }

    /// Store `v` unless an equal member is present, and answer whether it
    /// was stored.
    pub(in crate::value) fn insert(&self, v: Value) -> bool {
        self.0.borrow_mut().insert(v)
    }

    /// Remove the member equal to `v`, and answer the member the store held.
    pub(in crate::value) fn take(&self, v: &Value) -> Option<Value> {
        self.0.borrow_mut().take(v)
    }
}
