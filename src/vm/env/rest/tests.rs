// audited: 2026-10-06
//! Unit tests for the rest-list builder: one region per cell, or one region for the list.
//!
//! docs/impl/region/restlist.md

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::value::arena::region_of;
use crate::value::fiberheap::FiberHeap;
use crate::value::RestListLayout;

/// The region of each cell of `list`, head first, and the elements in order.
/// Panics on a list that does not end in the empty list.
fn cells(heap: &FiberHeap, list: Value) -> (Vec<RuntimeRegion>, Vec<Value>) {
    let mut regions = Vec::new();
    let mut elements = Vec::new();
    let mut cur = list;
    while let Some(pair) = cur.as_pair() {
        regions.push(region_of(heap, cur).expect("a cell lives in a region"));
        elements.push(pair.first);
        cur = pair.rest;
    }
    assert!(cur.is_empty_list(), "a rest list ends in the empty list");
    (regions, elements)
}

/// A heap element in a region of its own, holding its birth reference.
fn element(heap: &mut FiberHeap) -> (Value, RuntimeRegion) {
    let region = heap.new_runtime_region();
    let v = crate::value::build::pair(heap, Value::int(7), Value::EMPTY_LIST, region);
    (v, region)
}

#[test]
fn one_region_layout_builds_every_cell_into_one_region() {
    let mut vm = VM::new();
    let heap = vm.heap();
    let args = [Value::int(1), Value::int(2), Value::int(3)];
    let list = VM::args_to_list(&args, RestListLayout::OneRegion, heap);
    let (regions, elements) = cells(heap, list);
    assert_eq!(elements, args.to_vec(), "the elements read back in order");
    assert!(
        regions.iter().all(|r| *r == regions[0]),
        "every cell shares the head's region: {regions:?}"
    );
    assert_eq!(
        heap.region_rc(regions[0]),
        1,
        "the region holds one reference, the head's"
    );
}

#[test]
fn one_region_layout_frees_every_cell_with_the_head() {
    let mut vm = VM::new();
    let heap = vm.heap();
    let before = heap.active_region_count();
    let args = [Value::int(1), Value::int(2), Value::int(3), Value::int(4)];
    let list = VM::args_to_list(&args, RestListLayout::OneRegion, heap);
    let head = region_of(heap, list).expect("the head lives in a region");
    assert_eq!(
        heap.active_region_count(),
        before + 1,
        "one region for the list"
    );
    heap.decref_region(head);
    assert_eq!(
        heap.active_region_count(),
        before,
        "the head's release frees the whole list"
    );
}

/// Counterfactual: a list region that counted no edge to an element would leave
/// the element's count at its birth reference, and one whose free skipped the
/// edges would leave it raised after the list is gone.
#[test]
fn one_region_layout_counts_each_element_once_per_cell() {
    let mut vm = VM::new();
    let heap = vm.heap();
    let (e, er) = element(heap);
    assert_eq!(heap.region_rc(er), 1, "the element's birth reference");
    let args = [e, Value::int(2), e];
    let list = VM::args_to_list(&args, RestListLayout::OneRegion, heap);
    assert_eq!(
        heap.region_rc(er),
        3,
        "each of the two cells that hold the element counts it"
    );
    let head = region_of(heap, list).expect("the head lives in a region");
    heap.decref_region(head);
    assert_eq!(
        heap.region_rc(er),
        1,
        "the list's free releases both edges and leaves the birth reference"
    );
}

#[test]
fn per_cell_layout_builds_each_cell_into_a_region_of_its_own() {
    let mut vm = VM::new();
    let heap = vm.heap();
    let args = [Value::int(1), Value::int(2), Value::int(3)];
    let list = VM::args_to_list(&args, RestListLayout::PerCell, heap);
    let (regions, elements) = cells(heap, list);
    assert_eq!(elements, args.to_vec(), "the elements read back in order");
    let mut distinct = regions.clone();
    distinct.sort_by_key(|r| r.get());
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "three cells, three regions: {regions:?}");
    for r in &regions {
        assert_eq!(
            heap.region_rc(*r),
            1,
            "each cell's region holds one reference: the head's is the caller's, \
             every other is the edge from the cell before it"
        );
    }
}

#[test]
fn an_empty_rest_list_mints_nothing_under_either_layout() {
    for layout in [RestListLayout::PerCell, RestListLayout::OneRegion] {
        let mut vm = VM::new();
        let heap = vm.heap();
        let before = heap.active_region_count();
        let list = VM::args_to_list(&[], layout, heap);
        assert!(
            list.is_empty_list(),
            "{layout:?}: no arguments, the empty list"
        );
        assert_eq!(
            heap.active_region_count(),
            before,
            "{layout:?}: no region for an empty list"
        );
    }
}
