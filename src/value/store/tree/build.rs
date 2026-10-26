// audited: 2026-10-26
//! A tree loaded from its entries in order, one level at a time, into exactly the nodes they need.
//!
//! docs/impl/mutable.md

use super::node::{Node, CAPACITY};
use super::{Key, Tree, Val};
use crate::value::store::extent::Extent;

impl<K: Key, V: Val> Tree<K, V> {
    /// A tree holding `entries`, whose keys ascend and are distinct, in an
    /// extent of exactly its nodes that `room` claims. An empty tree calls no
    /// `room`.
    pub(in crate::value) fn new(
        entries: impl IntoIterator<Item = (K, V)>,
        room: impl FnOnce(usize) -> *mut Node<K, V>,
    ) -> Self {
        let mut level: Vec<(K, V)> = entries.into_iter().collect();
        debug_assert!(
            level.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "a tree is built from ascending, distinct keys"
        );
        if level.is_empty() {
            return Tree::empty();
        }
        let len = level.len();
        let mut nodes: Vec<Node<K, V>> = Vec::new();
        let mut children: Option<Vec<u32>> = None;
        let mut height = 0;
        loop {
            let (next, parents) = Self::load_level(&mut nodes, &level, children.as_deref());
            if next.is_empty() {
                let root = parents[0];
                return Tree::from_nodes(Extent::new(&nodes, room), root, height, len);
            }
            level = next;
            children = Some(parents);
            height += 1;
        }
    }

    /// Spread `level` over the fewest nodes that hold it with one entry between
    /// each two of them, and push those nodes. Answer the entries between them,
    /// which go up a level, and the new nodes' indices. A node of an internal
    /// level takes one more of `children` than it takes entries.
    fn load_level(
        nodes: &mut Vec<Node<K, V>>,
        level: &[(K, V)],
        children: Option<&[u32]>,
    ) -> (Vec<(K, V)>, Vec<u32>) {
        let count = (level.len() + 1).div_ceil(CAPACITY + 1);
        let held = level.len() + 1 - count;
        let (each, extra) = (held / count, held % count);
        let mut between = Vec::with_capacity(count - 1);
        let mut indices = Vec::with_capacity(count);
        let (mut at, mut child) = (0, 0);
        for n in 0..count {
            let take = each + usize::from(n < extra);
            let kids = children.map(|c| &c[child..child + take + 1]);
            indices.push(nodes.len() as u32);
            nodes.push(Node::holding(&level[at..at + take], kids));
            at += take;
            child += take + 1;
            if n + 1 < count {
                between.push(level[at]);
                at += 1;
            }
        }
        (between, indices)
    }
}
