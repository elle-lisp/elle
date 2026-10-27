// audited: 2026-10-26
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
#[cfg(test)]
mod tests;

use std::cell::Cell;

use super::extent::{Extent, ExtentRead};
use crate::value::types::TableKey;
use crate::value::Value;
pub use insert::Put;
pub use iter::Iter;
pub use node::Node;
use node::NONE;

/// More levels than a tree has. A tree of `u32` node indices is at most
/// thirteen levels deep, because every node below the root has six children
/// or more.
const MAX_DEPTH: usize = 16;

/// A key a tree keeps its entries in the order of.
pub trait Key: Copy + Ord + 'static {
    /// Whether a node finds a key by comparing its keys in order from the
    /// first, rather than by a binary search.
    const SCANS: bool;
}

impl Key for Value {
    const SCANS: bool = false;
}

impl Key for TableKey {
    const SCANS: bool = true;
}

/// What a tree keeps under each key: a `@struct`'s value, or nothing for an
/// `@set`'s member.
pub trait Val: Copy + 'static {}

impl<V: Copy + 'static> Val for V {}

/// The entries of a sorted store, each a key and its value. The root is
/// `NONE` when there are none, and a height of 0 makes the root a leaf.
pub struct Tree<K, V> {
    nodes: Extent<Node<K, V>>,
    root: Cell<u32>,
    height: Cell<u32>,
    free: Cell<u32>,
    len: Cell<usize>,
}

/// A read of a tree. A write to it panics while one is live.
pub struct TreeRead<'a, K, V> {
    nodes: ExtentRead<'a, Node<K, V>>,
    root: u32,
    len: usize,
}

/// Where a key lands: on the entry at a place of a node, or at the leaf place
/// a new entry takes, whose insert needs `need` nodes. The leaf of an empty
/// tree is `NONE`.
enum Spot {
    Found(u32, usize),
    Vacant { at: u32, i: usize, need: usize },
}

/// Descend from `root` toward `key`, calling `step` with each node passed and
/// the place taken within it. Answer the node and place the descent ended at,
/// and whether the entry under `key` lies there. Without one, the descent
/// ends at the leaf place `key` would take. The tree holds at least one entry.
#[inline]
fn descend<K: Key, V: Val>(
    nodes: &[Node<K, V>],
    root: u32,
    key: &K,
    mut step: impl FnMut(&Node<K, V>, usize),
) -> (u32, usize, bool) {
    let mut at = root;
    loop {
        let node = &nodes[at as usize];
        let (i, found) = match node.search(key) {
            Ok(i) => (i, true),
            Err(i) => (i, false),
        };
        step(node, i);
        if found || node.is_leaf() {
            return (at, i, found);
        }
        at = node.child(i);
    }
}

impl<K: Key, V: Val> Tree<K, V> {
    /// A tree with no entries, which claims no extent.
    pub fn empty() -> Self {
        Tree::from_nodes(Extent::empty(), NONE, 0, 0)
    }

    fn from_nodes(nodes: Extent<Node<K, V>>, root: u32, height: u32, len: usize) -> Self {
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
    pub fn read(&self) -> TreeRead<'_, K, V> {
        self.try_read().expect("already mutably borrowed")
    }

    /// Read the entries, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<TreeRead<'_, K, V>> {
        Some(TreeRead {
            nodes: self.nodes.try_read()?,
            root: self.root.get(),
            len: self.len.get(),
        })
    }

    /// Make the free list hold `need` nodes, appending blank ones to the
    /// extent through `room` for those it lacks. This is the one write that
    /// claims, and it edits no entry.
    fn reserve(&self, need: usize, room: impl FnOnce(usize) -> *mut Node<K, V>) {
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

    /// Descend toward `key`, and answer where it lands. A split climbs only
    /// through the run of full nodes that ends at the leaf, and splits the
    /// root when the run starts there.
    #[inline]
    fn locate(&self, key: &K) -> Spot {
        if self.len.get() == 0 {
            return Spot::Vacant {
                at: NONE,
                i: 0,
                need: 1,
            };
        }
        let nodes = self.nodes.read();
        let mut full = 0;
        let step = |node: &Node<K, V>, _| {
            full = if node.len() == node::CAPACITY {
                full + 1
            } else {
                0
            };
        };
        let (at, i, found) = descend(&nodes, self.root.get(), key, step);
        if found {
            return Spot::Found(at, i);
        }
        let depth = self.height.get() as usize + 1;
        let need = full + usize::from(full == depth);
        Spot::Vacant { at, i, need }
    }

    /// Take a node off the free list. `reserve` has put one there.
    fn take_free(&self, nodes: &[Node<K, V>]) -> u32 {
        let at = self.free.get();
        assert!(at != NONE, "a tree edit takes only the nodes it reserved");
        self.free.set(nodes[at as usize].next_free());
        at
    }

    /// Put the node at `at` on the free list.
    fn release(&self, nodes: &mut [Node<K, V>], at: u32) {
        nodes[at as usize].free(self.free.get());
        self.free.set(at);
    }
}

impl<'a, K: Key, V: Val> TreeRead<'a, K, V> {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The value under `key`.
    pub fn get(&self, key: &K) -> Option<&V> {
        if self.len == 0 {
            return None;
        }
        let (at, i, found) = descend(&self.nodes, self.root, key, |_, _| {});
        found.then(|| self.nodes[at as usize].val(i))
    }

    /// The entries in key order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter::new(&self.nodes, self.root, self.len)
    }

    /// The bytes of the extent the nodes lie in, free nodes included.
    #[inline]
    pub fn node_bytes(&self) -> usize {
        std::mem::size_of_val::<[Node<K, V>]>(&self.nodes)
    }
}
