// audited: 2026-10-26
//! A tree's nodes and writes: a scan stops, keys lie side by side, an insert takes what it reserves, and a child names its parent.
//!
//! docs/impl/mutable.md

use std::cell::Cell;
use std::cmp::Ordering;

use super::node::{Node, CAPACITY, NONE};
use super::{Key, Tree, Val};
use crate::value::fiberheap::FiberHeap;
use crate::value::types::TableKey;
use crate::value::Value;

thread_local! {
    /// The compares the `Counted` keys on this thread have made.
    static COMPARES: Cell<usize> = const { Cell::new(0) };
}

/// A key that counts each compare it makes, in a node that scans.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Counted(i64);

impl PartialOrd for Counted {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Counted {
    fn cmp(&self, other: &Self) -> Ordering {
        COMPARES.with(|n| n.set(n.get() + 1));
        self.0.cmp(&other.0)
    }
}

impl Key for Counted {
    const SCANS: bool = true;
}

// A binary search answers every place a scan does, so only the count of
// compares tells them apart: a binary search over a full node compares about
// four times wherever the key lands.
#[test]
fn a_scanning_node_compares_each_entry_up_to_its_answer() {
    let keys: Vec<i64> = (0..CAPACITY as i64).map(|i| 2 * i).collect();
    let entries: Vec<(Counted, ())> = keys.iter().map(|&k| (Counted(k), ())).collect();
    let node: Node<Counted, ()> = Node::holding(&entries, None);
    for probe in -1..=2 * CAPACITY as i64 {
        let want = keys.binary_search(&probe);
        COMPARES.with(|n| n.set(0));
        assert_eq!(node.search(&Counted(probe)), want, "the place of {probe}");
        let compares = match want {
            Ok(i) => i + 1,
            Err(i) => (i + 1).min(CAPACITY),
        };
        assert_eq!(
            COMPARES.with(Cell::get),
            compares,
            "the compares that place {probe}"
        );
    }
}

/// Whether a node keyed on `K` scans.
fn scans<K: Key>() -> bool {
    K::SCANS
}

#[test]
fn a_struct_node_scans_and_a_set_node_takes_a_binary_search() {
    assert!(scans::<TableKey>(), "a @struct node takes a binary search");
    assert!(!scans::<Value>(), "an @set node scans");
}

/// The bytes from one item to the next.
fn stride<T>(first: &T, second: &T) -> usize {
    second as *const T as usize - first as *const T as usize
}

// The counter-factual: a node that interleaves each key with its value
// answers every search, edit and walk the same, so every model test passes.
// Only where the keys lie tells the two apart: interleaved, a scan steps over
// a value between each two keys it compares.
#[test]
fn a_struct_node_keeps_its_keys_side_by_side() {
    let entries: Vec<(TableKey, Value)> = (0..CAPACITY as i64)
        .map(|i| (TableKey::Int(i), Value::int(i)))
        .collect();
    let node: Node<TableKey, Value> = Node::holding(&entries, None);
    assert_eq!(
        stride(node.key(0), node.key(1)),
        std::mem::size_of::<TableKey>(),
        "the step from one key to the next"
    );
    assert_eq!(
        stride(node.val(0), node.val(1)),
        std::mem::size_of::<Value>(),
        "the step from one value to the next"
    );
}

/// Walk `tree` from its root, and check that the root names no parent and
/// that every other node names its parent and its place there.
fn check_links<K: Key, V: Val>(tree: &Tree<K, V>, after: &str) {
    let nodes = tree.nodes.read();
    let root = tree.root.get();
    if root == NONE {
        return;
    }
    assert_eq!(
        nodes[root as usize].parent().0,
        NONE,
        "the root's parent after {after}"
    );
    let mut level = vec![root];
    for _ in 0..tree.height.get() {
        let mut below = Vec::new();
        for &at in &level {
            let node = &nodes[at as usize];
            for place in 0..=node.len() {
                let child = node.child(place);
                assert_eq!(
                    nodes[child as usize].parent(),
                    (at, place),
                    "the link of node {child} after {after}"
                );
                below.push(child);
            }
        }
        level = below;
    }
}

/// The `i`th key in a scattered order, distinct for the first million.
fn scattered(i: i64) -> Value {
    Value::int(i * 7919 % 1_000_003)
}

#[test]
fn a_built_tree_links_each_child_to_its_parent() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();
    let entries = (0..3000).map(|i| (Value::int(i), ()));
    let tree: Tree<Value, ()> = Tree::new(entries, |n| heap.alloc_room_in_region(n, region));
    assert!(tree.height.get() >= 3, "the build fills four levels");
    check_links(&tree, "the build");
}

// The counter-factual: an edit that leaves a link stale passes the model
// tests until some later climb reads that link, and the climb may never come.
// The check after every write names the first write that leaves one stale. The
// deletes, in another scattered order, borrow from both siblings and merge at
// every level, and the last one empties the root.
#[test]
fn every_write_keeps_each_child_linked_to_its_parent() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();
    let tree: Tree<Value, ()> = Tree::empty();
    for i in 0..3000 {
        assert!(tree.insert((scattered(i), ()), |n| heap.alloc_room_in_region(n, region)));
        check_links(&tree, &format!("the insert of key {i}"));
    }
    assert!(tree.height.get() >= 3, "the inserts fill four levels");
    for i in 0..3000 {
        let key = scattered(i * 1237 % 3000);
        assert!(tree.take(&key).is_some(), "the delete of key {i}");
        check_links(&tree, &format!("the delete of key {i}"));
    }
    assert_eq!(tree.root.get(), NONE, "every key is deleted");
}

// The counter-factual: an insert that reserves a node too many passes every
// model test, because the spare node waits on the free list and a later split
// takes it. A count of every full node on the path, where only the run that
// ends at the leaf splits, does that whenever a full node sits above one with
// room. A tree three levels deep meets that path on scattered keys.
#[test]
fn an_insert_leaves_no_node_on_the_free_list() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();
    let tree: Tree<Value, ()> = Tree::empty();
    for i in 0..5000 {
        assert!(tree.insert((scattered(i), ()), |n| heap.alloc_room_in_region(n, region)));
        assert_eq!(
            tree.free.get(),
            NONE,
            "the insert of key {i} left a spare node"
        );
    }
    assert!(tree.height.get() >= 3, "the tree splits below its root");
}
