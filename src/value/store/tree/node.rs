// audited: 2026-10-23
//! A node of a sorted store's B-tree: up to eleven entries in order, and the index of each child.
//!
//! docs/impl/mutable.md
//!
//! A leaf's child indices are never read, so an edit moves them with the
//! entries and nothing keeps them meaningful. A free node's first child index
//! names the next free node.

use std::mem::MaybeUninit;

use super::Entry;

/// The tree's order: every node but the root holds `MIN_LEN` to `CAPACITY`
/// entries, the shape std's `BTreeMap` uses.
const B: usize = 6;
pub(super) const CAPACITY: usize = 2 * B - 1;
pub(super) const MIN_LEN: usize = B - 1;

/// The index no node has: the end of the free list, and the root of an empty
/// tree.
pub(super) const NONE: u32 = u32::MAX;

/// A node of the tree. Its first `len` entries are written.
pub struct Node<T> {
    len: u32,
    entries: [MaybeUninit<T>; CAPACITY],
    children: [u32; CAPACITY + 1],
}

impl<T: Copy> Clone for Node<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Node<T> {}

impl<T: Entry> Node<T> {
    /// A node with no entries.
    pub(super) fn blank() -> Self {
        Node {
            len: 0,
            entries: [MaybeUninit::uninit(); CAPACITY],
            children: [NONE; CAPACITY + 1],
        }
    }

    /// A node holding `entries` over `children`, one more child than entries
    /// when the node is internal.
    pub(super) fn holding(entries: &[T], children: Option<&[u32]>) -> Self {
        let mut node = Node::blank();
        for (slot, e) in node.entries.iter_mut().zip(entries) {
            slot.write(*e);
        }
        if let Some(children) = children {
            node.children[..children.len()].copy_from_slice(children);
        }
        node.len = entries.len() as u32;
        node
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len as usize
    }

    /// The written entries, in order.
    #[inline]
    pub(super) fn entries(&self) -> &[T] {
        // SAFETY: the first `len` entries are written, and `MaybeUninit<T>`
        // has the layout of `T`.
        unsafe { &*(&self.entries[..self.len()] as *const [MaybeUninit<T>] as *const [T]) }
    }

    #[inline]
    pub(super) fn child(&self, i: usize) -> u32 {
        self.children[i]
    }

    /// The place of the entry under `key`, or where it would go.
    #[inline]
    pub(super) fn search(&self, key: &T::Key) -> Result<usize, usize> {
        self.entries().binary_search_by(|e| e.key().cmp(key))
    }

    /// Put `e` at `i`, with `right` as the child after it. The node is not
    /// full.
    pub(super) fn insert(&mut self, i: usize, e: T, right: u32) {
        let len = self.len();
        debug_assert!(len < CAPACITY && i <= len);
        self.entries.copy_within(i..len, i + 1);
        self.entries[i].write(e);
        self.children.copy_within(i + 1..len + 1, i + 2);
        self.children[i + 1] = right;
        self.len += 1;
    }

    /// Remove the entry at `i` and the child after it, and answer both.
    pub(super) fn remove(&mut self, i: usize) -> (T, u32) {
        let len = self.len();
        let taken = (self.entries()[i], self.children[i + 1]);
        self.entries.copy_within(i + 1..len, i);
        self.children.copy_within(i + 2..len + 1, i + 1);
        self.len -= 1;
        taken
    }

    /// Store `e` at `i`, and answer the entry it displaced.
    pub(super) fn replace(&mut self, i: usize, e: T) -> T {
        std::mem::replace(&mut self.entries_mut()[i], e)
    }

    /// Put `e` first, with `left` as the child before it.
    pub(super) fn push_front(&mut self, e: T, left: u32) {
        let len = self.len();
        self.entries.copy_within(0..len, 1);
        self.entries[0].write(e);
        self.children.copy_within(0..len + 1, 1);
        self.children[0] = left;
        self.len += 1;
    }

    /// Remove the first entry and the child before it, and answer both.
    pub(super) fn pop_front(&mut self) -> (T, u32) {
        let len = self.len();
        let taken = (self.entries()[0], self.children[0]);
        self.entries.copy_within(1..len, 0);
        self.children.copy_within(1..len + 1, 0);
        self.len -= 1;
        taken
    }

    /// Put `e` last, with `right` as the child after it. The node is not
    /// full.
    pub(super) fn push_back(&mut self, e: T, right: u32) {
        let len = self.len();
        self.insert(len, e, right);
    }

    /// Remove the last entry and the child after it, and answer both.
    pub(super) fn pop_back(&mut self) -> (T, u32) {
        let len = self.len();
        self.remove(len - 1)
    }

    /// Append `separator`, then every entry and child of `right`.
    pub(super) fn absorb(&mut self, separator: T, right: &Node<T>) {
        let (len, more) = (self.len(), right.len());
        debug_assert!(len + 1 + more <= CAPACITY);
        self.entries[len].write(separator);
        self.entries[len + 1..len + 1 + more].copy_from_slice(&right.entries[..more]);
        self.children[len + 1..len + 2 + more].copy_from_slice(&right.children[..more + 1]);
        self.len += 1 + more as u32;
    }

    /// Split this full node around its middle while `e` goes in at `i` with
    /// `right` after it. This node keeps the lower half, and the upper half
    /// and the middle entry come back.
    pub(super) fn split(&mut self, i: usize, e: T, right: u32) -> (T, Node<T>) {
        debug_assert!(self.len() == CAPACITY);
        let mut entries = [MaybeUninit::uninit(); CAPACITY + 1];
        entries[..i].copy_from_slice(&self.entries[..i]);
        entries[i].write(e);
        entries[i + 1..].copy_from_slice(&self.entries[i..]);
        let mut children = [NONE; CAPACITY + 2];
        children[..i + 1].copy_from_slice(&self.children[..i + 1]);
        children[i + 1] = right;
        children[i + 2..].copy_from_slice(&self.children[i + 1..]);
        let mut upper = Node::blank();
        upper.entries[..CAPACITY - B].copy_from_slice(&entries[B + 1..]);
        upper.children[..CAPACITY - B + 1].copy_from_slice(&children[B + 1..]);
        upper.len = (CAPACITY - B) as u32;
        self.entries[..B].copy_from_slice(&entries[..B]);
        self.children[..B + 1].copy_from_slice(&children[..B + 1]);
        self.len = B as u32;
        // SAFETY: the middle entry is one of the twelve written above.
        (unsafe { entries[B].assume_init() }, upper)
    }

    /// The free node after this one.
    #[inline]
    pub(super) fn next_free(&self) -> u32 {
        self.children[0]
    }

    /// Empty this node and link it before `next` on the free list.
    pub(super) fn free(&mut self, next: u32) {
        self.len = 0;
        self.children[0] = next;
    }

    fn entries_mut(&mut self) -> &mut [T] {
        let len = self.len();
        // SAFETY: as for `entries`.
        unsafe { &mut *(&mut self.entries[..len] as *mut [MaybeUninit<T>] as *mut [T]) }
    }
}
