// audited: 2026-10-27
//! A node's edits and splits: a leaf moves its keys and values alone, and an internal node moves each child with its entry.
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

// The counter-factual: a split that gives the upper half no leaf mark, moves
// the lower half's child indices, or builds the upper half as a new node and
// copies it over the free one, passes every model test for the same reason.
// Both halves keep their marks, and the upper half, once a free internal
// node, edits as a leaf.
#[test]
fn a_leaf_splits_into_two_leaves() {
    let ks: Vec<i64> = (1..=CAPACITY as i64).map(|k| 2 * k).collect();
    let mut lower = marked_leaf(&ks);
    let mut upper = node(&[], Some(&marks()));
    let middle = lower.split(&mut upper, 0, entry(0), NONE);
    assert_eq!(middle.0, Value::int(2 * B as i64), "the middle key");
    unmoved(&lower, &[0, 2, 4, 6, 8, 10], "a split, below");
    assert!(upper.is_leaf(), "the upper half of a split leaf");
    unmoved(&upper, &[14, 16, 18, 20, 22], "a split, above");
    upper.insert(0, entry(13), NONE);
    unmoved(&upper, &[13, 14, 16, 18, 20, 22], "an insert above a split");
}

/// A node holding the keys `ks`, each with its negation as its value, over
/// `children` when it is internal.
fn valued(ks: &[i64], children: Option<&[u32]>) -> Node<Value, Value> {
    let entries: Vec<_> = ks.iter().map(|&k| valued_entry(k)).collect();
    Node::holding(&entries, children)
}

/// The entry under the key `k`, whose value is its negation.
fn valued_entry(k: i64) -> (Value, Value) {
    (Value::int(k), Value::int(-k))
}

/// The entries of `node`, in order.
fn entries(node: &Node<Value, Value>) -> Vec<(Value, Value)> {
    node.keys()
        .iter()
        .copied()
        .zip(node.vals().iter().copied())
        .collect()
}

// Wherever the new entry lands, a split of a full node orders the twelve
// entries, keeps the first six below, sends the seventh up, and moves the
// last five, each value with its key and each child beside its entry. The
// free node the upper half fills was a node of the other kind.
#[test]
fn a_split_at_every_place_keeps_the_twelve_in_order() {
    let ks: Vec<i64> = (1..=CAPACITY as i64).map(|k| 2 * k).collect();
    let children: Vec<u32> = (100..=100 + CAPACITY as u32).collect();
    for i in 0..=CAPACITY {
        let k = 2 * i as i64 + 1;
        let mut order: Vec<_> = ks.iter().map(|&key| valued_entry(key)).collect();
        order.insert(i, valued_entry(k));
        let mut kids = children.clone();
        kids.insert(i + 1, 300);
        for internal in [false, true] {
            let mut lower = valued(&ks, internal.then_some(&children[..]));
            let mut upper = valued(&[], (!internal).then_some(&[NONE][..]));
            let middle = lower.split(&mut upper, i, valued_entry(k), 300);
            let at = format!("a split at {i}, internal {internal}");
            assert_eq!(middle, order[B], "the middle entry of {at}");
            assert_eq!(entries(&lower), order[..B], "the lower half of {at}");
            assert_eq!(entries(&upper), order[B + 1..], "the upper half of {at}");
            assert_eq!(upper.is_leaf(), !internal, "the upper kind of {at}");
            if internal {
                let (below, above) = (&lower.children[..=B], &upper.children[..=CAPACITY - B]);
                assert_eq!(below, &kids[..=B], "the lower children of {at}");
                assert_eq!(above, &kids[B + 1..], "the upper children of {at}");
            }
        }
    }
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
    let mut upper = node(&[], None);
    lower.split(&mut upper, 0, entry(0), 300);
    assert_eq!(lower.children[..B + 1], [100, 300, 101, 102, 103, 104, 105]);
    upper.insert(0, entry(13), 400);
    assert_eq!(upper.children[..7], [106, 400, 107, 108, 109, 110, 111]);
}
