// audited: 2026-10-26
//! A node's edits: a leaf moves its keys and values alone, and an internal node moves each child with its entry.
//!
//! docs/impl/mutable.md

use super::{Node, B, CAPACITY, NONE};
use crate::value::Value;

/// Child indices that no edit of a leaf writes.
fn marks() -> [u32; CAPACITY + 1] {
    std::array::from_fn(|j| 100 + j as u32)
}

/// The entry under the key `k`.
fn entry(k: i64) -> (Value, ()) {
    (Value::int(k), ())
}

/// A node holding the keys `ks`, over `children` when it is internal.
fn node(ks: &[i64], children: Option<&[u32]>) -> Node<Value, ()> {
    let entries: Vec<_> = ks.iter().map(|&k| entry(k)).collect();
    Node::holding(&entries, children)
}

/// A leaf holding the keys `ks`, its child indices marked.
fn marked_leaf(ks: &[i64]) -> Node<Value, ()> {
    let mut leaf = node(ks, None);
    leaf.children = marks();
    leaf
}

/// The keys of `node`, in order.
fn keys(node: &Node<Value, ()>) -> Vec<i64> {
    node.keys()
        .iter()
        .map(|k| k.as_int().expect("an int key"))
        .collect()
}

/// Check that `leaf` holds the keys `want` with its marks where they lay.
fn unmoved(leaf: &Node<Value, ()>, want: &[i64], edit: &str) {
    assert_eq!(keys(leaf), want, "the keys after {edit}");
    assert_eq!(leaf.children, marks(), "the child indices after {edit}");
}

// The counter-factual: a leaf edit that moves the child indices with the
// entries passes every model test, because no read of a leaf takes a child.
// Only the indices tell the two apart: marked, they stay where they lie
// through every edit of a leaf, and a leaf's removal answers no child.
#[test]
fn a_leaf_edit_moves_its_entries_and_no_child_index() {
    let mut leaf = marked_leaf(&[0, 2, 4, 6, 8, 10]);
    leaf.insert(1, entry(1), NONE);
    unmoved(&leaf, &[0, 1, 2, 4, 6, 8, 10], "an insert");
    leaf.push_front(entry(-2), NONE);
    unmoved(&leaf, &[-2, 0, 1, 2, 4, 6, 8, 10], "a push at the front");
    leaf.push_back(entry(12), NONE);
    unmoved(&leaf, &[-2, 0, 1, 2, 4, 6, 8, 10, 12], "a push at the back");
    assert_eq!(leaf.remove(2).1, NONE, "the child a removal answers");
    unmoved(&leaf, &[-2, 0, 2, 4, 6, 8, 10, 12], "a removal");
    assert_eq!(
        leaf.pop_front().1,
        NONE,
        "the child a pop at the front answers"
    );
    unmoved(&leaf, &[0, 2, 4, 6, 8, 10, 12], "a pop at the front");
    assert_eq!(
        leaf.pop_back().1,
        NONE,
        "the child a pop at the back answers"
    );
    unmoved(&leaf, &[0, 2, 4, 6, 8, 10], "a pop at the back");
    leaf.absorb(entry(11), &marked_leaf(&[14, 16]));
    unmoved(&leaf, &[0, 2, 4, 6, 8, 10, 11, 14, 16], "a merge");
}

// The counter-factual: a split that gives the upper half no leaf mark, or
// moves the lower half's child indices, passes every model test for the same
// reason. The lower half keeps its marks, and the upper half edits as a leaf.
#[test]
fn a_leaf_splits_into_two_leaves() {
    let ks: Vec<i64> = (1..=CAPACITY as i64).map(|k| 2 * k).collect();
    let mut lower = marked_leaf(&ks);
    let (middle, mut upper) = lower.split(0, entry(0), NONE);
    assert_eq!(middle.0, Value::int(2 * B as i64), "the middle key");
    unmoved(&lower, &[0, 2, 4, 6, 8, 10], "a split, below");
    upper.children = marks();
    upper.insert(0, entry(13), NONE);
    unmoved(&upper, &[13, 14, 16, 18, 20, 22], "an insert above a split");
}

// An internal node holds one more child than entries, and each edit carries
// the child beside the entry it moves. The upper half of a split internal
// node is internal too.
#[test]
fn an_internal_edit_moves_each_child_with_its_entry() {
    let children: Vec<u32> = (100..107).collect();
    let mut inner = node(&[0, 2, 4, 6, 8, 10], Some(&children));
    inner.insert(1, entry(1), 200);
    assert_eq!(
        inner.children[..8],
        [100, 101, 200, 102, 103, 104, 105, 106]
    );
    assert_eq!(inner.remove(1).1, 200, "the child after the removed entry");
    assert_eq!(inner.pop_front().1, 100, "the child before the first entry");
    assert_eq!(inner.children[..6], [101, 102, 103, 104, 105, 106]);
    let ks: Vec<i64> = (1..=CAPACITY as i64).map(|k| 2 * k).collect();
    let children: Vec<u32> = (100..=100 + CAPACITY as u32).collect();
    let mut lower = node(&ks, Some(&children));
    let (_, mut upper) = lower.split(0, entry(0), 300);
    assert_eq!(lower.children[..B + 1], [100, 300, 101, 102, 103, 104, 105]);
    upper.insert(0, entry(13), 400);
    assert_eq!(upper.children[..7], [106, 400, 107, 108, 109, 110, 111]);
}
