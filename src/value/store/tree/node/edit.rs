// audited: 2026-10-26
//! A node's edits: an entry goes in, comes out, or moves to a sibling, with the child beside it.
//!
//! docs/impl/mutable.md
//!
//! A leaf's child indices are never read, so an edit moves them with the
//! entries and nothing keeps them meaningful.

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
        self.children.copy_within(i + 1..len + 1, i + 2);
        self.children[i + 1] = right;
        self.len += 1;
    }

    /// Remove the entry at `i` and the child after it, and answer both.
    pub(in crate::value::store::tree) fn remove(&mut self, i: usize) -> ((K, V), u32) {
        let len = self.len();
        let taken = (self.entry(i), self.children[i + 1]);
        self.shift(i + 1..len, i);
        self.children.copy_within(i + 2..len + 1, i + 1);
        self.len -= 1;
        taken
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
        self.children.copy_within(0..len + 1, 1);
        self.children[0] = left;
        self.len += 1;
    }

    /// Remove the first entry and the child before it, and answer both.
    pub(in crate::value::store::tree) fn pop_front(&mut self) -> ((K, V), u32) {
        let len = self.len();
        let taken = (self.entry(0), self.children[0]);
        self.shift(1..len, 0);
        self.children.copy_within(1..len + 1, 0);
        self.len -= 1;
        taken
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

    /// Append `separator`, then every entry and child of `right`.
    pub(in crate::value::store::tree) fn absorb(&mut self, separator: (K, V), right: &Node<K, V>) {
        let (len, more) = (self.len(), right.len());
        debug_assert!(len + 1 + more <= CAPACITY);
        self.write(len, separator);
        self.keys[len + 1..len + 1 + more].copy_from_slice(&right.keys[..more]);
        self.vals[len + 1..len + 1 + more].copy_from_slice(&right.vals[..more]);
        self.children[len + 1..len + 2 + more].copy_from_slice(&right.children[..more + 1]);
        self.len += 1 + more as u32;
    }

    /// Split this full node around its middle while `e` goes in at `i` with
    /// `right` after it. This node keeps the lower half, and the upper half
    /// and the middle entry come back.
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
        let children: [_; CAPACITY + 2] = spliced(&self.children, i + 1, right, NONE);
        let mut upper = Node::blank();
        upper.keys[..CAPACITY - B].copy_from_slice(&keys[B + 1..]);
        upper.vals[..CAPACITY - B].copy_from_slice(&vals[B + 1..]);
        upper.children[..CAPACITY - B + 1].copy_from_slice(&children[B + 1..]);
        upper.len = (CAPACITY - B) as u32;
        self.keys[..B].copy_from_slice(&keys[..B]);
        self.vals[..B].copy_from_slice(&vals[..B]);
        self.children[..B + 1].copy_from_slice(&children[..B + 1]);
        self.len = B as u32;
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
}
