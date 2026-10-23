// audited: 2026-10-22
//! The `@array` store: an extent of values in the container's region, and the edits its funnels make.
//!
//! docs/impl/mutable.md

use super::extent::{no_room, Extent, ExtentRead};
use crate::value::Value;

/// The contents of an `@array`.
pub type ArrayStore = Extent<Value>;

/// A read of an `@array`'s elements. A write to the store panics while one
/// is live.
pub type ArrayRead<'a> = ExtentRead<'a, Value>;

/// The writes that grow call `room` for a larger extent, as
/// `Extent::splice` does. The rest never grow.
impl Extent<Value> {
    pub(in crate::value) fn push(&self, v: Value, room: impl FnOnce(usize) -> *mut Value) {
        self.append(&[v], room);
    }

    pub(in crate::value) fn extend(&self, vs: &[Value], room: impl FnOnce(usize) -> *mut Value) {
        self.append(vs, room);
    }

    /// Remove the last `n` elements, fewer if the store is shorter, and
    /// answer them in order.
    pub(in crate::value) fn drain_tail(&self, n: usize) -> Vec<Value> {
        let len = self.read().len();
        self.splice(len - n.min(len)..len, &[], no_room, <[Value]>::to_vec)
    }

    /// Panics when `index` is past the end, as `Vec::insert` does.
    pub(in crate::value) fn insert(
        &self,
        index: usize,
        v: Value,
        room: impl FnOnce(usize) -> *mut Value,
    ) {
        self.splice(index..index, &[v], room, |_| ());
    }

    /// Panics when `index` is out of bounds, as `Vec::remove` does.
    pub(in crate::value) fn remove(&self, index: usize) -> Value {
        self.splice(index..index + 1, &[], no_room, |removed| removed[0])
    }
}
