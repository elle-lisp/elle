// audited: 2026-10-26
//! A walk over a tree's entries in key order, from either end, that climbs through the parent links.
//!
//! docs/impl/mutable.md

use super::node::{Node, NONE};
use super::{Key, Val};

/// The entries of a tree in key order, each a key and its value. Each end is
/// a node and a place between two of its entries: the front's next entry
/// lies after it, and the back's before it. A count of the entries left stops
/// both ends where they meet, so no entry comes back twice.
pub struct Iter<'a, K, V> {
    nodes: &'a [Node<K, V>],
    front: (u32, usize),
    back: (u32, usize),
    left: usize,
}

impl<'a, K: Key, V: Val> Iter<'a, K, V> {
    pub(super) fn new(nodes: &'a [Node<K, V>], root: u32, len: usize) -> Self {
        let mut it = Iter {
            nodes,
            front: (NONE, 0),
            back: (NONE, 0),
            left: len,
        };
        if len > 0 {
            it.front = it.start_of(root);
            it.back = it.end_of(root);
        }
        it
    }

    /// The place before the first entry of the subtree at `at`.
    fn start_of(&self, mut at: u32) -> (u32, usize) {
        while !self.nodes[at as usize].is_leaf() {
            at = self.nodes[at as usize].child(0);
        }
        (at, 0)
    }

    /// The place after the last entry of the subtree at `at`.
    fn end_of(&self, mut at: u32) -> (u32, usize) {
        loop {
            let node = &self.nodes[at as usize];
            if node.is_leaf() {
                return (at, node.len());
            }
            at = node.child(node.len());
        }
    }
}

impl<'a, K: Key, V: Val> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<(&'a K, &'a V)> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let (mut at, mut i) = self.front;
        while i == self.nodes[at as usize].len() {
            (at, i) = self.nodes[at as usize].parent();
        }
        let node = &self.nodes[at as usize];
        self.front = if node.is_leaf() {
            (at, i + 1)
        } else {
            self.start_of(node.child(i + 1))
        };
        Some((node.key(i), node.val(i)))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<'a, K: Key, V: Val> DoubleEndedIterator for Iter<'a, K, V> {
    fn next_back(&mut self) -> Option<(&'a K, &'a V)> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let (mut at, mut i) = self.back;
        while i == 0 {
            (at, i) = self.nodes[at as usize].parent();
        }
        let node = &self.nodes[at as usize];
        self.back = if node.is_leaf() {
            (at, i - 1)
        } else {
            self.end_of(node.child(i - 1))
        };
        Some((node.key(i - 1), node.val(i - 1)))
    }
}

impl<K: Key, V: Val> ExactSizeIterator for Iter<'_, K, V> {}
