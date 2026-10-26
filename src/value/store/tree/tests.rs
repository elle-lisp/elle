// audited: 2026-10-26
//! A tree's insert reserves exactly the nodes its splits take, so a tree that only grows keeps no free node.
//!
//! docs/impl/mutable.md
//!
//! The counter-factual: an insert that reserves a node too many passes every
//! model test, because the spare node waits on the free list and a later split
//! takes it. A count of every full node on the path, where only the run that
//! ends at the leaf splits, does that whenever a full node sits above one with
//! room. A tree three levels deep meets that path on scattered keys.

use super::node::NONE;
use super::Tree;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

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
