// audited: 2026-10-23
//! A walk over a tree's entries in key order, from either end, with a path from the root to each cursor.
//!
//! docs/impl/mutable.md

use super::node::Node;
use super::{Entry, Path};

/// The entries of a tree in key order. A count of the entries left stops
/// both ends where they meet, so no entry comes back twice.
pub struct Iter<'a, T> {
    nodes: &'a [Node<T>],
    height: usize,
    front: Path,
    back: Path,
    left: usize,
}

impl<'a, T: Entry> Iter<'a, T> {
    pub(super) fn new(nodes: &'a [Node<T>], root: u32, height: u32, len: usize) -> Self {
        let mut it = Iter {
            nodes,
            height: height as usize,
            front: Path::new(),
            back: Path::new(),
            left: len,
        };
        if len > 0 {
            it.descend_front(root);
            it.descend_back(root);
        }
        it
    }

    /// Push the path from `at` to the first entry of its subtree.
    fn descend_front(&mut self, mut at: u32) {
        loop {
            self.front.push(at, 0);
            if self.front.depth > self.height {
                return;
            }
            at = self.nodes[at as usize].child(0);
        }
    }

    /// Push the path from `at` to the last entry of its subtree. A step
    /// names the place just past the next entry to answer.
    fn descend_back(&mut self, mut at: u32) {
        loop {
            let len = self.nodes[at as usize].len();
            self.back.push(at, len);
            if self.back.depth > self.height {
                return;
            }
            at = self.nodes[at as usize].child(len);
        }
    }
}

impl<'a, T: Entry> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        loop {
            let (at, i) = self.front.at(self.front.depth - 1);
            let node = &self.nodes[at as usize];
            if i < node.len() {
                self.front.set_place(self.front.depth - 1, i + 1);
                if self.front.depth <= self.height {
                    self.descend_front(node.child(i + 1));
                }
                return Some(&node.entries()[i]);
            }
            self.front.depth -= 1;
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<'a, T: Entry> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<&'a T> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        loop {
            let (at, i) = self.back.at(self.back.depth - 1);
            let node = &self.nodes[at as usize];
            if i > 0 {
                self.back.set_place(self.back.depth - 1, i - 1);
                if self.back.depth <= self.height {
                    self.descend_back(node.child(i - 1));
                }
                return Some(&node.entries()[i - 1]);
            }
            self.back.depth -= 1;
        }
    }
}

impl<T: Entry> ExactSizeIterator for Iter<'_, T> {}
