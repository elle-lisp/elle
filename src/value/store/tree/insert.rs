// audited: 2026-10-23
//! A tree's insert and put: a new entry goes into its leaf, and each full node on the way up splits.
//!
//! docs/impl/mutable.md

use super::node::{Node, CAPACITY, NONE};
use super::{locate, Entry, Path, Tree};

/// What a put did: ran its edit over the entry under the key, answering what
/// the edit answered, or stored a new entry, answering that entry.
pub enum Put<R, T> {
    Edited(R),
    Stored(T),
}

/// Where a key lands: on the entry found at a level of the path, or at the
/// leaf place the path ends at, which an insert there reaches through the
/// nodes it needs.
enum Spot {
    Found(Path, usize),
    Vacant(Path, usize),
}

impl<T: Entry> Tree<T> {
    /// Store `e` at its place in the order unless an entry under an equal key
    /// is present, and answer whether it was stored. An insert whose splits
    /// need more nodes than the free list holds calls `room` for a larger
    /// extent, as `Extent::splice` does, before it edits anything.
    pub(in crate::value) fn insert(&self, e: T, room: impl FnOnce(usize) -> *mut Node<T>) -> bool {
        let Spot::Vacant(path, need) = self.spot(e.key()) else {
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
        match self.spot(key) {
            Spot::Found(path, level) => {
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
            Spot::Vacant(path, need) => {
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

    /// Where an entry under `key` lands, and for a vacant spot the nodes an
    /// insert there needs.
    fn spot(&self, key: &T::Key) -> Spot {
        if self.len.get() == 0 {
            return Spot::Vacant(Path::new(), 1);
        }
        let nodes = self.nodes.read();
        let (path, found) = locate(&nodes, self.root.get(), self.height.get(), key);
        if let Some(level) = found {
            return Spot::Found(path, level);
        }
        let full = (0..path.depth)
            .rev()
            .take_while(|&level| nodes[path.at(level).0 as usize].len() == CAPACITY)
            .count();
        let root_splits = full == path.depth;
        Spot::Vacant(path, full + usize::from(root_splits))
    }
}
