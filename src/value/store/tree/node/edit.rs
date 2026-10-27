// audited: 2026-10-26
//! A node's edits: an entry goes in, comes out, or moves to a sibling, with the child beside it.
//!
//! docs/impl/mutable.md
//!
//! A leaf's child indices are never read, so an edit of a leaf leaves them
//! where they lie and answers `NONE` for a child it removes.

use std::mem::MaybeUninit;
use std::ops::Range;

use super::{Node, B, CAPACITY, NONE};
use crate::value::store::tree::{Key, Val};

/// `from` with `item` put in at `at`, one item longer, which is `N`.
#[inline]
fn spliced<T: Copy, const N: usize>(from: &[T], at: usize, item: T, fill: T) -> [T; N] {
    let mut out = [fill; N];
    out[..at].copy_from_slice(&from[..at]);
    out[at] = item;
    out[at + 1..].copy_from_slice(&from[at..]);
    out
}

impl<K: Key, V: Val> Node<K, V> {
    /// Put `e` at `i`, with `right` as the child after it. The node is not
    /// full.
    pub(in crate::value::store::tree) fn insert(&mut self, i: usize, e: (K, V), right: u32) {
        let len = self.len();
        debug_assert!(len < CAPACITY && i <= len);
        self.shift(i..len, i + 1);
        self.write(i, e);
        self.insert_child(i + 1, len, right);
        self.len += 1;
    }

    /// Remove the entry at `i` and the child after it, and answer both.
    pub(in crate::value::store::tree) fn remove(&mut self, i: usize) -> ((K, V), u32) {
        let len = self.len();
        let entry = self.entry(i);
        self.shift(i + 1..len, i);
        let child = self.remove_child(i + 1, len);
        self.len -= 1;
        (entry, child)
    }

    /// Store `e` at `i`, and answer the entry it displaced.
    pub(in crate::value::store::tree) fn replace(&mut self, i: usize, e: (K, V)) -> (K, V) {
        let displaced = self.entry(i);
        self.write(i, e);
        displaced
    }

    /// Put `e` first, with `left` as the child before it.
    pub(in crate::value::store::tree) fn push_front(&mut self, e: (K, V), left: u32) {
        let len = self.len();
        self.shift(0..len, 1);
        self.write(0, e);
        self.insert_child(0, len, left);
        self.len += 1;
    }

    /// Remove the first entry and the child before it, and answer both.
    pub(in crate::value::store::tree) fn pop_front(&mut self) -> ((K, V), u32) {
        let len = self.len();
        let entry = self.entry(0);
        self.shift(1..len, 0);
        let child = self.remove_child(0, len);
        self.len -= 1;
        (entry, child)
    }

    /// Put `e` last, with `right` as the child after it. The node is not
    /// full.
    pub(in crate::value::store::tree) fn push_back(&mut self, e: (K, V), right: u32) {
        let len = self.len();
        self.insert(len, e, right);
    }

    /// Remove the last entry and the child after it, and answer both.
    pub(in crate::value::store::tree) fn pop_back(&mut self) -> ((K, V), u32) {
        let len = self.len();
        self.remove(len - 1)
    }

    /// Append `separator`, then every entry and child of `right`, a sibling
    /// on the same level.
    pub(in crate::value::store::tree) fn absorb(&mut self, separator: (K, V), right: &Node<K, V>) {
        let (len, more) = (self.len(), right.len());
        debug_assert!(len + 1 + more <= CAPACITY && self.leaf == right.leaf);
        self.write(len, separator);
        self.keys[len + 1..len + 1 + more].copy_from_slice(&right.keys[..more]);
        self.vals[len + 1..len + 1 + more].copy_from_slice(&right.vals[..more]);
        if !self.leaf {
            self.children[len + 1..len + 2 + more].copy_from_slice(&right.children[..more + 1]);
        }
        self.len += 1 + more as u8;
    }

    /// Split this full node around its middle while `e` goes in at `i` with
    /// `right` after it. This node keeps the lower half, and the upper half,
    /// a leaf when this node is one, and the middle entry come back.
    pub(in crate::value::store::tree) fn split(
        &mut self,
        i: usize,
        e: (K, V),
        right: u32,
    ) -> ((K, V), Node<K, V>) {
        debug_assert!(self.len() == CAPACITY);
        let keys: [_; CAPACITY + 1] =
            spliced(&self.keys, i, MaybeUninit::new(e.0), MaybeUninit::uninit());
        let vals: [_; CAPACITY + 1] =
            spliced(&self.vals, i, MaybeUninit::new(e.1), MaybeUninit::uninit());
        let mut upper = Node::blank();
        upper.leaf = self.leaf;
        upper.keys[..CAPACITY - B].copy_from_slice(&keys[B + 1..]);
        upper.vals[..CAPACITY - B].copy_from_slice(&vals[B + 1..]);
        upper.len = (CAPACITY - B) as u8;
        if !self.leaf {
            let children: [_; CAPACITY + 2] = spliced(&self.children, i + 1, right, NONE);
            upper.children[..CAPACITY - B + 1].copy_from_slice(&children[B + 1..]);
            self.children[..B + 1].copy_from_slice(&children[..B + 1]);
        }
        self.keys[..B].copy_from_slice(&keys[..B]);
        self.vals[..B].copy_from_slice(&vals[..B]);
        self.len = B as u8;
        // SAFETY: the middle entry is one of the twelve written above.
        let middle = unsafe { (keys[B].assume_init(), vals[B].assume_init()) };
        (middle, upper)
    }

    /// The written entry at `i`.
    #[inline]
    fn entry(&self, i: usize) -> (K, V) {
        (self.keys()[i], self.vals()[i])
    }

    /// Move the keys and values at `from` to start at `to`.
    #[inline]
    fn shift(&mut self, from: Range<usize>, to: usize) {
        self.keys.copy_within(from.clone(), to);
        self.vals.copy_within(from, to);
    }

    /// Put `child` at `at` among the children of this node, which held `len`
    /// entries before the edit.
    #[inline]
    fn insert_child(&mut self, at: usize, len: usize, child: u32) {
        if !self.leaf {
            self.children.copy_within(at..len + 1, at + 1);
            self.children[at] = child;
        }
    }

    /// Take the child at `at` from this node, which held `len` entries before
    /// the edit, and answer it.
    #[inline]
    fn remove_child(&mut self, at: usize, len: usize) -> u32 {
        if self.leaf {
            return NONE;
        }
        let child = self.children[at];
        self.children.copy_within(at + 1..len + 1, at);
        child
    }
}
