// audited: 2026-10-26
//! A tree's delete: the entry leaves its node, and each node left short refills from a sibling or merges.
//!
//! docs/impl/mutable.md

use super::node::{Node, MIN_LEN, NONE};
use super::{Entry, Path, Spot, Tree};

impl<T: Entry> Tree<T> {
    /// Remove the entry under `key`, and answer the entry the tree held. A
    /// delete claims nothing.
    pub(in crate::value) fn take(&self, key: &T::Key) -> Option<T> {
        let mut path = Path::new();
        let Spot::Found(found) = self.locate(key, &mut path) else {
            return None;
        };
        let taken = self.nodes.with_slice_mut(|nodes| {
            let taken = self.detach(nodes, &mut path, found);
            self.refill(nodes, &path);
            taken
        });
        self.len.set(self.len.get() - 1);
        Some(taken)
    }

    /// Take the entry `path` found at `found` out of the tree, and extend
    /// `path` to the leaf that lost an entry. An entry in an internal node
    /// trades places with its predecessor, the last entry of the leaf at the
    /// right edge of the subtree before it.
    fn detach(&self, nodes: &mut [Node<T>], path: &mut Path, found: usize) -> T {
        let height = self.height.get() as usize;
        let (at, i) = path.at(found);
        if found == height {
            return nodes[at as usize].remove(i).0;
        }
        let mut below = nodes[at as usize].child(i);
        for level in found + 1..=height {
            let last = nodes[below as usize].len();
            path.push(below, last);
            if level < height {
                below = nodes[below as usize].child(last);
            }
        }
        let (predecessor, _) = nodes[below as usize].pop_back();
        nodes[at as usize].replace(i, predecessor)
    }

    /// Restore the minimum fill from the leaf at the end of `path` up to the
    /// root, after an entry left that leaf.
    fn refill(&self, nodes: &mut [Node<T>], path: &Path) {
        for level in (1..path.depth).rev() {
            let node = path.at(level).0 as usize;
            if nodes[node].len() >= MIN_LEN {
                return;
            }
            let (parent, edge) = path.at(level - 1);
            let parent = parent as usize;
            let left = (edge > 0).then(|| nodes[parent].child(edge - 1) as usize);
            let right =
                (edge < nodes[parent].len()).then(|| nodes[parent].child(edge + 1) as usize);
            if let Some(left) = left.filter(|&l| nodes[l].len() > MIN_LEN) {
                let (e, child) = nodes[left].pop_back();
                let separator = nodes[parent].replace(edge - 1, e);
                nodes[node].push_front(separator, child);
                return;
            }
            if let Some(right) = right.filter(|&r| nodes[r].len() > MIN_LEN) {
                let (e, child) = nodes[right].pop_front();
                let separator = nodes[parent].replace(edge, e);
                nodes[node].push_back(separator, child);
                return;
            }
            // Neither sibling can spare an entry, so the node and one sibling
            // fit in one node with the parent's entry between them.
            let (into, at) = match left {
                Some(left) => (left, edge - 1),
                None => (node, edge),
            };
            let (separator, merged) = nodes[parent].remove(at);
            let absorbed = nodes[merged as usize];
            nodes[into].absorb(separator, &absorbed);
            self.release(nodes, merged);
        }
        let root = self.root.get();
        if nodes[root as usize].len() > 0 {
            return;
        }
        let height = self.height.get();
        if height == 0 {
            self.root.set(NONE);
        } else {
            self.root.set(nodes[root as usize].child(0));
            self.height.set(height - 1);
        }
        self.release(nodes, root);
    }
}
