// audited: 2026-10-26
//! A tree's insert and put: a new entry goes into its leaf, and each full node on the way up splits.
//!
//! docs/impl/mutable.md

use super::node::{Node, CAPACITY, NONE};
use super::{Entry, Path, Spot, Tree};

/// What a put did: ran its edit over the entry under the key, answering what
/// the edit answered, or stored a new entry, answering that entry.
pub enum Put<R, T> {
    Edited(R),
    Stored(T),
}

impl<T: Entry> Tree<T> {
    /// Store `e` at its place in the order unless an entry under an equal key
    /// is present, and answer whether it was stored. An insert whose splits
    /// need more nodes than the free list holds calls `room` for a larger
    /// extent, as `Extent::splice` does, before it edits anything.
    pub(in crate::value) fn insert(&self, e: T, room: impl FnOnce(usize) -> *mut Node<T>) -> bool {
        let mut path = Path::new();
        let Spot::Vacant(need) = self.locate(e.key(), &mut path) else {
            return false;
        };
        self.reserve(need, room);
        self.store_at(&path, e);
        true
    }

    /// Run `edit` over the entry under `key` in place when one is present;
    /// `edit` must leave the entry's key as it found it, and claims nothing.
    /// Otherwise store the entry `vacant` makes, whose key equals `key`, and
    /// claim the nodes it needs through the room `vacant` hands back with it,
    /// as [`Self::insert`] does. One descent serves either.
    pub(in crate::value) fn put<R, F>(
        &self,
        key: &T::Key,
        edit: impl FnOnce(&mut T) -> R,
        vacant: impl FnOnce() -> (T, F),
    ) -> Put<R, T>
    where
        F: FnOnce(usize) -> *mut Node<T>,
    {
        let mut path = Path::new();
        match self.locate(key, &mut path) {
            Spot::Found(level) => {
                let (at, i) = path.at(level);
                Put::Edited(self.nodes.with_slice_mut(|nodes| {
                    let node = &mut nodes[at as usize];
                    let mut e = node.entries()[i];
                    let answer = edit(&mut e);
                    debug_assert!(e.key() == key, "an edit keeps the entry's key");
                    node.replace(i, e);
                    answer
                }))
            }
            Spot::Vacant(need) => {
                let (e, room) = vacant();
                debug_assert!(e.key() == key, "a put stores the key it sought");
                self.reserve(need, room);
                self.store_at(&path, e);
                Put::Stored(e)
            }
        }
    }

    /// Store `e` at the leaf place `path` ends at, splitting each full node on
    /// the way up. The free list holds every node the splits take.
    fn store_at(&self, path: &Path, e: T) {
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
    }
}
