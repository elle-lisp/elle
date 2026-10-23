// audited: 2026-10-23
//! A sorted store's B-tree: its entries in key order, in nodes that lie in one extent of the container's region.
//!
//! docs/impl/mutable.md
//!
//! A child is its node's index in the extent, so a growth that moves the
//! extent moves no link. A write runs inside the extent's own write guard, and
//! the tree's other fields change only there, so a read guard sees them fixed.

mod build;
mod insert;
mod iter;
mod node;
mod remove;

use std::cell::Cell;

use super::extent::{Extent, ExtentRead};
use crate::value::Value;
pub use iter::Iter;
pub use node::Node;
use node::NONE;

/// The deepest path from the root to a leaf. A tree of `u32` node indices is
/// at most thirteen levels deep, because every node below the root has six
/// children or more.
const MAX_DEPTH: usize = 16;

/// An entry a tree keeps in the order of its key.
pub trait Entry: Copy + 'static {
    type Key: Ord + ?Sized;
    fn key(&self) -> &Self::Key;
}

impl Entry for Value {
    type Key = Value;

    #[inline]
    fn key(&self) -> &Value {
        self
    }
}

/// The entries of a sorted store. The root is `NONE` when there are none,
/// and a height of 0 makes the root a leaf.
pub struct Tree<T> {
    nodes: Extent<Node<T>>,
    root: Cell<u32>,
    height: Cell<u32>,
    free: Cell<u32>,
    len: Cell<usize>,
}

/// A read of a tree. A write to it panics while one is live.
pub struct TreeRead<'a, T> {
    nodes: ExtentRead<'a, Node<T>>,
    root: u32,
    height: u32,
    len: usize,
}

/// The node at each level from the root down, with the place a search or a
/// walk took within it.
#[derive(Clone, Copy)]
struct Path {
    steps: [(u32, usize); MAX_DEPTH],
    depth: usize,
}

impl Path {
    fn new() -> Self {
        Path {
            steps: [(NONE, 0); MAX_DEPTH],
            depth: 0,
        }
    }

    fn push(&mut self, node: u32, at: usize) {
        self.steps[self.depth] = (node, at);
        self.depth += 1;
    }

    fn at(&self, level: usize) -> (u32, usize) {
        self.steps[level]
    }
}

impl<T: Entry> Tree<T> {
    /// A tree with no entries, which claims no extent.
    pub fn empty() -> Self {
        Tree::from_nodes(Extent::empty(), NONE, 0, 0)
    }

    fn from_nodes(nodes: Extent<Node<T>>, root: u32, height: u32, len: usize) -> Self {
        Tree {
            nodes,
            root: Cell::new(root),
            height: Cell::new(height),
            free: Cell::new(NONE),
            len: Cell::new(len),
        }
    }

    /// Read the entries. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> TreeRead<'_, T> {
        self.try_read().expect("already mutably borrowed")
    }

    /// Read the entries, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<TreeRead<'_, T>> {
        Some(TreeRead {
            nodes: self.nodes.try_read()?,
            root: self.root.get(),
            height: self.height.get(),
            len: self.len.get(),
        })
    }

    /// Make the free list hold `need` nodes, appending blank ones to the
    /// extent through `room` for those it lacks. This is the one write that
    /// claims, and it edits no entry.
    fn reserve(&self, need: usize, room: impl FnOnce(usize) -> *mut Node<T>) {
        let (have, start) = {
            let nodes = self.nodes.read();
            let mut have = 0;
            let mut at = self.free.get();
            while have < need && at != NONE {
                have += 1;
                at = nodes[at as usize].next_free();
            }
            (have, nodes.len())
        };
        if have == need {
            return;
        }
        let blanks = [Node::blank(); MAX_DEPTH + 1];
        let added = need - have;
        assert!(
            start + added < NONE as usize,
            "a sorted store holds fewer than 2^32 nodes"
        );
        self.nodes.append(&blanks[..added], room);
        self.nodes.with_slice_mut(|nodes| {
            for at in start..start + added {
                self.release(nodes, at as u32);
            }
        });
    }

    /// Take a node off the free list. `reserve` has put one there.
    fn take_free(&self, nodes: &[Node<T>]) -> u32 {
        let at = self.free.get();
        assert!(at != NONE, "a tree edit takes only the nodes it reserved");
        self.free.set(nodes[at as usize].next_free());
        at
    }

    /// Put the node at `at` on the free list.
    fn release(&self, nodes: &mut [Node<T>], at: u32) {
        nodes[at as usize].free(self.free.get());
        self.free.set(at);
    }
}

impl<'a, T: Entry> TreeRead<'a, T> {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entry under `key`.
    pub fn get(&self, key: &T::Key) -> Option<&T> {
        if self.len == 0 {
            return None;
        }
        let mut at = self.root;
        for level in 0..=self.height {
            let node = &self.nodes[at as usize];
            match node.search(key) {
                Ok(i) => return Some(&node.entries()[i]),
                Err(i) if level < self.height => at = node.child(i),
                Err(_) => return None,
            }
        }
        None
    }

    /// The entries in key order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter::new(&self.nodes, self.root, self.height, self.len)
    }

    /// The bytes of the extent the nodes lie in, free nodes included.
    #[inline]
    pub fn node_bytes(&self) -> usize {
        std::mem::size_of_val::<[Node<T>]>(&self.nodes)
    }
}
