// audited: 2026-10-26
//! A tree's node search and insert: a scan stops at the place it answers, and an insert reserves exactly the nodes it takes.
//!
//! docs/impl/mutable.md

use std::cell::Cell;
use std::cmp::Ordering;

use super::node::{Node, CAPACITY, NONE};
use super::{Entry, Tree};
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

impl Entry for Counted {
    type Key = Counted;
    const SCANS: bool = true;

    fn key(&self) -> &Counted {
        self
    }
}

// A binary search answers every place a scan does, so only the count of
// compares tells them apart: a binary search over a full node compares about
// four times wherever the key lands.
#[test]
fn a_scanning_node_compares_each_entry_up_to_its_answer() {
    let keys: Vec<i64> = (0..CAPACITY as i64).map(|i| 2 * i).collect();
    let entries: Vec<Counted> = keys.iter().map(|&k| Counted(k)).collect();
    let node = Node::holding(&entries, None);
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

/// Whether a node of `T` scans.
fn scans<T: Entry>() -> bool {
    T::SCANS
}

#[test]
fn a_struct_node_scans_and_a_set_node_takes_a_binary_search() {
    assert!(
        scans::<(TableKey, Value)>(),
        "a @struct node takes a binary search"
    );
    assert!(!scans::<Value>(), "an @set node scans");
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
    let tree: Tree<Value> = Tree::empty();
    for i in 0..5000 {
        let key = Value::int(i * 7919 % 1_000_003);
        assert!(tree.insert(key, |n| heap.alloc_room_in_region(n, region)));
        assert_eq!(
            tree.free.get(),
            NONE,
            "the insert of key {i} left a spare node"
        );
    }
    assert!(tree.height.get() >= 3, "the tree splits below its root");
}
