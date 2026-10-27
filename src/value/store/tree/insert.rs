// audited: 2026-10-27
//! A tree's insert and put: a new entry goes into its leaf, and each full node on the way up splits.
//!
//! docs/impl/mutable.md

use super::node::{Node, B, CAPACITY, NONE};
use super::{Key, Spot, Tree, Val};

/// What a put did: ran its edit over the value under the key, answering what
/// the edit answered, or stored a new entry, answering that entry's key.
pub enum Put<R, K> {
    Edited(R),
    Stored(K),
}

impl<K: Key, V: Val> Tree<K, V> {
    /// Store `e` at its place in the order unless an entry under an equal key
    /// is present, and answer whether it was stored. An insert whose splits
    /// need more nodes than the free list holds calls `room` for a larger
    /// extent, as `Extent::splice` does, before it edits anything.
    pub(in crate::value) fn insert(
        &self,
        e: (K, V),
        room: impl FnOnce(usize) -> *mut Node<K, V>,
    ) -> bool {
        let Spot::Vacant { at, i, need } = self.locate(&e.0) else {
            return false;
        };
        self.reserve(need, room);
        self.store_at(at, i, e);
        true
    }

    /// Run `edit` over the value under `key` in place when one is present;
    /// `edit` claims nothing. Otherwise store the entry `vacant` makes, whose
    /// key equals `key`, and claim the nodes it needs through the room
    /// `vacant` hands back with it, as [`Self::insert`] does. One descent
    /// serves either.
    pub(in crate::value) fn put<R, F>(
        &self,
        key: &K,
        edit: impl FnOnce(&mut V) -> R,
        vacant: impl FnOnce() -> ((K, V), F),
    ) -> Put<R, K>
    where
        F: FnOnce(usize) -> *mut Node<K, V>,
    {
        match self.locate(key) {
            Spot::Found(at, i) => Put::Edited(
                self.nodes
                    .with_slice_mut(|nodes| edit(nodes[at as usize].val_mut(i))),
            ),
            Spot::Vacant { at, i, need } => {
                let (e, room) = vacant();
                debug_assert!(e.0 == *key, "a put stores the key it sought");
                self.reserve(need, room);
                self.store_at(at, i, e);
                Put::Stored(e.0)
            }
        }
    }

    /// Store `e` at the place `i` of the leaf `at`, splitting each full node
    /// on the way up. The free list holds every node the splits take.
    fn store_at(&self, mut at: u32, mut i: usize, e: (K, V)) {
        self.nodes.with_slice_mut(|nodes| {
            if at == NONE {
                let leaf = self.take_free(nodes);
                nodes[leaf as usize] = Node::holding(&[e], None);
                self.root.set(leaf);
                self.height.set(0);
                return;
            }
            let (mut e, mut right) = (e, NONE);
            loop {
                let node = &mut nodes[at as usize];
                if node.len() < CAPACITY {
                    node.insert(i, e, right);
                    let len = node.len();
                    Node::adopt(nodes, at, i + 1..=len);
                    return;
                }
                let mut upper = Node::blank();
                let middle = node.split(&mut upper, i, e, right);
                let (parent, place) = node.parent();
                right = self.take_free(nodes);
                nodes[right as usize] = upper;
                Node::adopt(nodes, right, 0..=CAPACITY - B);
                Node::adopt(nodes, at, i + 1..=B);
                e = middle;
                if parent == NONE {
                    break;
                }
                (at, i) = (parent, place);
            }
            let root = self.take_free(nodes);
            nodes[root as usize] = Node::holding(&[e], Some(&[at, right]));
            Node::adopt(nodes, root, 0..=1);
            self.root.set(root);
            self.height.set(self.height.get() + 1);
        });
        self.len.set(self.len.get() + 1);
    }
}
