// audited: 2026-10-26
//! A node of a sorted store's B-tree: up to eleven keys side by side, the value under each, and the index of each child.
//!
//! docs/impl/mutable.md
//!
//! A free node's first child index names the next free node.

mod edit;
#[cfg(test)]
mod tests;

use std::cmp::Ordering;
use std::mem::MaybeUninit;

use super::{Key, Val};

/// The tree's order: every node but the root holds `MIN_LEN` to `CAPACITY`
/// entries, the shape std's `BTreeMap` uses.
const B: usize = 6;
pub(super) const CAPACITY: usize = 2 * B - 1;
pub(super) const MIN_LEN: usize = B - 1;

/// The index no node has: the end of the free list, and the root of an empty
/// tree.
pub(super) const NONE: u32 = u32::MAX;

/// A node of the tree. Its first `len` keys and values are written, and a
/// leaf's child indices mean nothing. A descent reads the length, the keys and
/// one child, so the values lie after them.
#[repr(C)]
pub struct Node<K, V> {
    len: u32,
    /// Whether the node is a leaf. Behind the length, it takes bytes an
    /// 8-byte key leaves as padding.
    leaf: bool,
    keys: [MaybeUninit<K>; CAPACITY],
    children: [u32; CAPACITY + 1],
    vals: [MaybeUninit<V>; CAPACITY],
}

impl<K: Copy, V: Copy> Clone for Node<K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: Copy, V: Copy> Copy for Node<K, V> {}

/// The first `len` of `slots`.
///
/// # Safety
///
/// The first `len` of `slots` are written.
#[inline]
unsafe fn written<T>(slots: &[MaybeUninit<T>], len: usize) -> &[T] {
    // SAFETY: `MaybeUninit<T>` has the layout of `T`, and the caller has
    // written these.
    unsafe { &*(&slots[..len] as *const [MaybeUninit<T>] as *const [T]) }
}

impl<K: Key, V: Val> Node<K, V> {
    /// A node with no entries.
    pub(super) fn blank() -> Self {
        Node {
            len: 0,
            leaf: true,
            keys: [MaybeUninit::uninit(); CAPACITY],
            children: [NONE; CAPACITY + 1],
            vals: [MaybeUninit::uninit(); CAPACITY],
        }
    }

    /// A node holding `entries` over `children`, one more child than entries
    /// when the node is internal.
    pub(super) fn holding(entries: &[(K, V)], children: Option<&[u32]>) -> Self {
        let mut node = Node::blank();
        for (i, &e) in entries.iter().enumerate() {
            node.write(i, e);
        }
        if let Some(children) = children {
            node.children[..children.len()].copy_from_slice(children);
            node.leaf = false;
        }
        node.len = entries.len() as u32;
        node
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len as usize
    }

    /// The key of the entry at `i`.
    #[inline]
    pub(super) fn key(&self, i: usize) -> &K {
        &self.keys()[i]
    }

    /// The value of the entry at `i`.
    #[inline]
    pub(super) fn val(&self, i: usize) -> &V {
        &self.vals()[i]
    }

    /// The value of the entry at `i`, to edit in place.
    #[inline]
    pub(super) fn val_mut(&mut self, i: usize) -> &mut V {
        assert!(i < self.len(), "a value lies within its node");
        // SAFETY: the first `len` values are written.
        unsafe { self.vals[i].assume_init_mut() }
    }

    #[inline]
    pub(super) fn child(&self, i: usize) -> u32 {
        self.children[i]
    }

    /// The place of the entry under `key`, or where it would go.
    #[inline]
    pub(super) fn search(&self, key: &K) -> Result<usize, usize> {
        let keys = self.keys();
        if !K::SCANS {
            return keys.binary_search(key);
        }
        for (i, k) in keys.iter().enumerate() {
            match key.cmp(k) {
                Ordering::Greater => {}
                Ordering::Equal => return Ok(i),
                Ordering::Less => return Err(i),
            }
        }
        Err(keys.len())
    }

    /// The free node after this one.
    #[inline]
    pub(super) fn next_free(&self) -> u32 {
        self.children[0]
    }

    /// Empty this node and link it before `next` on the free list.
    pub(super) fn free(&mut self, next: u32) {
        self.len = 0;
        self.children[0] = next;
    }

    /// The written keys, in order.
    #[inline]
    fn keys(&self) -> &[K] {
        // SAFETY: the first `len` keys are written.
        unsafe { written(&self.keys, self.len()) }
    }

    /// The written values, in the order of their keys.
    #[inline]
    fn vals(&self) -> &[V] {
        // SAFETY: the first `len` values are written.
        unsafe { written(&self.vals, self.len()) }
    }

    /// Write `e` at `i`, which the caller counts within the length.
    #[inline]
    fn write(&mut self, i: usize, (k, v): (K, V)) {
        self.keys[i].write(k);
        self.vals[i].write(v);
    }
}
