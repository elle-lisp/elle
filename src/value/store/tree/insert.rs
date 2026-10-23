// audited: 2026-10-23
//! A tree's insert: the entry goes into its leaf, and each full node on the way up splits.
//!
//! docs/impl/mutable.md

use super::node::{Node, CAPACITY, NONE};
use super::{Entry, Path, Tree};

impl<T: Entry> Tree<T> {
    /// Store `e` at its place in the order unless an entry under an equal key
    /// is present, and answer whether it was stored. An insert whose splits
    /// need more nodes than the free list holds calls `room` for a larger
    /// extent, as `Extent::splice` does, before it edits anything.
    pub(in crate::value) fn insert(&self, e: T, room: impl FnOnce(usize) -> *mut Node<T>) -> bool {
        let Some((path, need)) = self.place(e.key()) else {
            return false;
        };
        self.reserve(need, room);
        self.nodes.with_slice_mut(|nodes| {
            if path.depth == 0 {
                let leaf = self.take_free(nodes);
                nodes[leaf as usize] = Node::holding(&[e], None);
                self.root.set(leaf);
                self.height.set(0);
                return;
            }
            let (mut e, mut right) = (e, NONE);
            for level in (0..path.depth).rev() {
                let (at, i) = path.at(level);
                let node = &mut nodes[at as usize];
                if node.len() < CAPACITY {
                    node.insert(i, e, right);
                    return;
                }
                let (middle, upper) = node.split(i, e, right);
                right = self.take_free(nodes);
                nodes[right as usize] = upper;
                e = middle;
            }
            let root = self.take_free(nodes);
            nodes[root as usize] = Node::holding(&[e], Some(&[self.root.get(), right]));
            self.root.set(root);
            self.height.set(self.height.get() + 1);
        });
        self.len.set(self.len.get() + 1);
        true
    }

    /// The path to the leaf place of `key`, and the nodes an insert there
    /// needs, or `None` when an entry under `key` is present.
    fn place(&self, key: &T::Key) -> Option<(Path, usize)> {
        let mut path = Path::new();
        if self.len.get() == 0 {
            return Some((path, 1));
        }
        let nodes = self.nodes.read();
        let height = self.height.get() as usize;
        let mut at = self.root.get();
        for level in 0..=height {
            let node = &nodes[at as usize];
            let i = node.search(key).err()?;
            path.push(at, i);
            if level < height {
                at = node.child(i);
            }
        }
        let full = (0..path.depth)
            .rev()
            .take_while(|&level| nodes[path.at(level).0 as usize].len() == CAPACITY)
            .count();
        let root_splits = full == path.depth;
        Some((path, full + usize::from(root_splits)))
    }
}
