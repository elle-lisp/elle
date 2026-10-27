// audited: 2026-10-27
//! A tree's delete: the entry leaves its node, and each node left short refills from a sibling or merges.
//!
//! docs/impl/mutable.md

use super::node::{Node, MIN_LEN, NONE};
use super::{Key, Spot, Tree, Val};

impl<K: Key, V: Val> Tree<K, V> {
    /// Remove the entry under `key`, and answer the entry the tree held. A
    /// delete claims nothing.
    pub(in crate::value) fn take(&self, key: &K) -> Option<(K, V)> {
        let Spot::Found(at, i) = self.locate(key) else {
            return None;
        };
        let taken = self.nodes.with_slice_mut(|nodes| {
            let (taken, leaf) = Self::detach(nodes, at, i);
            self.refill(nodes, leaf);
            taken
        });
        self.len.set(self.len.get() - 1);
        Some(taken)
    }

    /// Take the entry at place `i` of the node `at` out of the tree, and
    /// answer it with the leaf that lost an entry. An entry in an internal
    /// node trades places with its predecessor, the last entry of the leaf at
    /// the right edge of the subtree before it.
    fn detach(nodes: &mut [Node<K, V>], at: u32, i: usize) -> ((K, V), u32) {
        if nodes[at as usize].is_leaf() {
            return (nodes[at as usize].remove(i).0, at);
        }
        let mut below = nodes[at as usize].child(i);
        while !nodes[below as usize].is_leaf() {
            let node = &nodes[below as usize];
            below = node.child(node.len());
        }
        let (predecessor, _) = nodes[below as usize].pop_back();
        (nodes[at as usize].replace(i, predecessor), below)
    }

    /// Restore the minimum fill from `leaf` up to the root, after an entry
    /// left that leaf.
    fn refill(&self, nodes: &mut [Node<K, V>], leaf: u32) {
        let mut node = leaf;
        loop {
            let (parent, edge) = nodes[node as usize].parent();
            if parent == NONE {
                break;
            }
            let at = node as usize;
            if nodes[at].len() >= MIN_LEN {
                return;
            }
            let up = parent as usize;
            let left = (edge > 0).then(|| nodes[up].child(edge - 1));
            let right = (edge < nodes[up].len()).then(|| nodes[up].child(edge + 1));
            if let Some(left) = left.filter(|&l| nodes[l as usize].len() > MIN_LEN) {
                let (e, child) = nodes[left as usize].pop_back();
                let separator = nodes[up].replace(edge - 1, e);
                nodes[at].push_front(separator, child);
                let len = nodes[at].len();
                Node::adopt(nodes, node, 0..=len);
                return;
            }
            if let Some(right) = right.filter(|&r| nodes[r as usize].len() > MIN_LEN) {
                let (e, child) = nodes[right as usize].pop_front();
                let rest = nodes[right as usize].len();
                Node::adopt(nodes, right, 0..=rest);
                let separator = nodes[up].replace(edge, e);
                nodes[at].push_back(separator, child);
                let len = nodes[at].len();
                Node::adopt(nodes, node, len..=len);
                return;
            }
            // Neither sibling can spare an entry, so the node and one sibling
            // fit in one node with the parent's entry between them.
            let (into, place) = match left {
                Some(left) => (left, edge - 1),
                None => (node, edge),
            };
            let (separator, merged) = nodes[up].remove(place);
            let after = nodes[up].len();
            Node::adopt(nodes, parent, place + 1..=after);
            let [kept, absorbed] = nodes
                .get_disjoint_mut([into as usize, merged as usize])
                .expect("a node merges with its sibling");
            let before = kept.len();
            kept.absorb(separator, absorbed);
            let len = kept.len();
            Node::adopt(nodes, into, before + 1..=len);
            self.release(nodes, merged);
            node = parent;
        }
        if nodes[node as usize].len() > 0 {
            return;
        }
        let height = self.height.get();
        if height == 0 {
            self.root.set(NONE);
        } else {
            let root = nodes[node as usize].child(0);
            nodes[root as usize].orphan();
            self.root.set(root);
            self.height.set(height - 1);
        }
        self.release(nodes, node);
    }
}
