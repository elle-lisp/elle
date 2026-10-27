// audited: 2026-10-27
//! A node's edits: an entry goes in, comes out, or moves to a sibling or a free node, with the child beside it.
//!
//! docs/impl/mutable.md
//!
//! A leaf's child indices are never read, so an edit or a split of a leaf
//! leaves them where they lie, in both nodes of a split, and answers `NONE`
//! for a child it removes.

use std::cmp::Ordering;
use std::ops::Range;

use super::{Node, B, CAPACITY, NONE};
use crate::value::store::tree::{Key, Val};

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
    /// `right` after it. This node keeps the lower half, the free node
    /// `upper` takes the upper half, a leaf when this node is one, and the
    /// middle entry comes back. The upper half moves first, and then `e` goes
    /// into the half that holds its place.
    pub(in crate::value::store::tree) fn split(
        &mut self,
        upper: &mut Node<K, V>,
        i: usize,
        e: (K, V),
        right: u32,
    ) -> (K, V) {
        debug_assert!(self.len() == CAPACITY);
        // The first entry the upper half takes, and the middle entry: the
        // entry before `e`'s place, `e` itself, or the entry after it.
        let (from, middle) = match i.cmp(&B) {
            Ordering::Less => (B, self.entry(B - 1)),
            Ordering::Equal => (B, e),
            Ordering::Greater => (B + 1, self.entry(B)),
        };
        let moved = CAPACITY - from;
        upper.keys[..moved].copy_from_slice(&self.keys[from..]);
        upper.vals[..moved].copy_from_slice(&self.vals[from..]);
        upper.len = moved as u8;
        upper.leaf = self.leaf;
        if !self.leaf {
            // When `e` goes up, its right child heads the upper half.
            let head = usize::from(i == B);
            upper.children[head..=moved].copy_from_slice(&self.children[from + head..]);
            if i == B {
                upper.children[0] = right;
            }
        }
        match i.cmp(&B) {
            Ordering::Less => {
                self.len = (B - 1) as u8;
                self.insert(i, e, right);
            }
            Ordering::Equal => self.len = B as u8,
            Ordering::Greater => {
                self.len = B as u8;
                upper.insert(i - from, e, right);
            }
        }
        middle
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
