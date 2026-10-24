// audited: 2026-10-22
//! The `@array` store: a sequence of values, read as a slice.
//!
//! docs/impl/mutable.md

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use crate::value::Value;

/// The contents of an `@array`.
pub struct ArrayStore(Rc<RefCell<Vec<Value>>>);

/// A read of an `@array`'s elements. A write to the store panics while one
/// is live.
pub struct ArrayRead<'a>(Ref<'a, Vec<Value>>);

impl std::ops::Deref for ArrayRead<'_> {
    type Target = [Value];

    #[inline]
    fn deref(&self) -> &[Value] {
        &self.0
    }
}

impl ArrayStore {
    /// A store holding `elements`. The value it goes into counts their
    /// regions when it is allocated.
    pub fn new(elements: Vec<Value>) -> Self {
        ArrayStore(Rc::new(RefCell::new(elements)))
    }

    /// Read the elements. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> ArrayRead<'_> {
        ArrayRead(self.0.borrow())
    }

    /// Read the elements, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<ArrayRead<'_>> {
        self.0.try_borrow().ok().map(ArrayRead)
    }

    pub(in crate::value) fn push(&self, v: Value) {
        self.0.borrow_mut().push(v);
    }

    pub(in crate::value) fn pop(&self) -> Option<Value> {
        self.0.borrow_mut().pop()
    }

    pub(in crate::value) fn extend(&self, vs: &[Value]) {
        self.0.borrow_mut().extend_from_slice(vs);
    }

    /// Remove the last `n` elements, fewer if the store is shorter, and
    /// answer them in order.
    pub(in crate::value) fn drain_tail(&self, n: usize) -> Vec<Value> {
        let mut vec = self.0.borrow_mut();
        let len = vec.len();
        vec.drain(len - n.min(len)..).collect()
    }

    /// Panics when `index` is past the end, as `Vec::insert` does.
    pub(in crate::value) fn insert(&self, index: usize, v: Value) {
        self.0.borrow_mut().insert(index, v);
    }

    /// Panics when `index` is out of bounds, as `Vec::remove` does.
    pub(in crate::value) fn remove(&self, index: usize) -> Value {
        self.0.borrow_mut().remove(index)
    }

    /// Store `v` at `index` and answer the value it displaced. Panics when
    /// `index` is out of bounds.
    pub(in crate::value) fn replace(&self, index: usize, v: Value) -> Value {
        std::mem::replace(&mut self.0.borrow_mut()[index], v)
    }

    /// Run `f` over the elements in place. `f` can reorder them but cannot
    /// change how many there are.
    pub(in crate::value) fn with_slice_mut<R>(&self, f: impl FnOnce(&mut [Value]) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}
