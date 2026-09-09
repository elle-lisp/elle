// audited: 2026-09-08
// The image store milestone: dump a data-only value graph, hydrate it by
// private file mapping, and prove the mechanism end to end.
// docs/impl/image/plan.md
//
// An image is page bytes plus relocations, hydrated into an ordinary counted
// region. The submodules below hold the test plan's pins, and this file holds
// the graph every one of them dumps.

use std::collections::{BTreeMap, BTreeSet};

use elle::hir::region::RuntimeRegion;
use elle::value::fiberheap::FiberHeap;
use elle::value::{HeapObject, Pair, SymbolId, TableKey, Value};
use elle::SymbolTable;

/// The keyword and the symbol [`build_graph`] carries. Neither spelling is in
/// the static vocabulary, so only the image's name table can carry them to a
/// fresh instance.
const GRAPH_KEYWORD: &str = "spike";
const GRAPH_SYMBOL: &str = "image-graph-symbol";

// ── Graph builders (trait-less data values, one region) ─────────────

fn alloc_str(heap: &mut FiberHeap, region: RuntimeRegion, s: &str) -> Value {
    let slice = heap.alloc_region_slice_in_region(s.as_bytes(), region);
    heap.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: Value::NIL,
        },
        region,
    )
}

fn alloc_bytes(heap: &mut FiberHeap, region: RuntimeRegion, b: &[u8]) -> Value {
    let slice = heap.alloc_region_slice_in_region(b, region);
    heap.alloc_in_region(
        HeapObject::LBytes {
            data: slice,
            traits: Value::NIL,
        },
        region,
    )
}

fn alloc_pair(heap: &mut FiberHeap, region: RuntimeRegion, first: Value, rest: Value) -> Value {
    heap.alloc_in_region(HeapObject::Pair(Pair::new(first, rest)), region)
}

fn alloc_array(heap: &mut FiberHeap, region: RuntimeRegion, items: &[Value]) -> Value {
    let slice = heap.alloc_region_slice_in_region(items, region);
    heap.alloc_in_region(
        HeapObject::LArray {
            elements: slice,
            traits: Value::NIL,
        },
        region,
    )
}

/// An immutable set: a sorted array of values, inline in the region. The
/// builder sorts, because a set's whole contract is that its elements are in
/// `Value` order — a probe is a binary search.
fn alloc_set(heap: &mut FiberHeap, region: RuntimeRegion, items: &[Value]) -> Value {
    let sorted: Vec<Value> = items.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let slice = heap.alloc_region_slice_in_region(&sorted, region);
    heap.alloc_in_region(
        HeapObject::LSet {
            data: slice,
            traits: Value::NIL,
        },
        region,
    )
}

/// An immutable struct: entries sorted by key, inline in the region.
fn alloc_struct(heap: &mut FiberHeap, region: RuntimeRegion, entries: &[(TableKey, Value)]) -> Value {
    let sorted: Vec<(TableKey, Value)> = entries
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect();
    let slice = heap.alloc_region_slice_in_region(&sorted, region);
    heap.alloc_in_region(
        HeapObject::LStruct {
            data: slice,
            traits: Value::NIL,
        },
        region,
    )
}

/// A representative data graph: nesting, every supported heap variant, and
/// supported immediates (ints, inline floats, bools, nil, keywords, symbols).
fn build_graph(heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
    let s = alloc_str(heap, region, "hello image");
    let b = alloc_bytes(heap, region, &[0xE1, 0x1E, 0x5C]);
    let inner = alloc_array(
        heap,
        region,
        &[
            Value::int(7),
            s,
            Value::keyword(GRAPH_KEYWORD),
            Value::float(2.5),
        ],
    );
    // A `Bool` key is the widest-padding key there is: one payload byte in a
    // slot sized for a `Value`, so the entry a wholesale copy would write
    // carries 23 bytes of whatever its construction temporary held.
    let table = alloc_struct(
        heap,
        region,
        &[
            (TableKey::Bool(true), Value::int(11)),
            (TableKey::Symbol(SymbolId::of(GRAPH_SYMBOL)), s),
            (TableKey::keyword(GRAPH_KEYWORD), Value::EMPTY_LIST),
        ],
    );
    let members = alloc_set(
        heap,
        region,
        &[
            Value::int(3),
            Value::keyword(GRAPH_KEYWORD),
            Value::symbol(SymbolId::of(GRAPH_SYMBOL)),
            s,
        ],
    );
    let tail = alloc_pair(heap, region, Value::bool(true), Value::EMPTY_LIST);
    let named = alloc_pair(heap, region, Value::symbol(SymbolId::of(GRAPH_SYMBOL)), tail);
    let listed = alloc_pair(heap, region, members, named);
    let sorted = alloc_pair(heap, region, table, listed);
    let mid = alloc_pair(heap, region, inner, sorted);
    let mid2 = alloc_pair(heap, region, b, mid);
    alloc_pair(heap, region, Value::int(1), mid2)
}

/// The memo a dumping instance of [`build_graph`]'s graph would hold: the two
/// spellings its values carry, and nothing else.
fn graph_names() -> SymbolTable {
    let mut names = SymbolTable::new();
    names.keyword(GRAPH_KEYWORD);
    names.intern(GRAPH_SYMBOL);
    names
}

/// Build [`build_graph`]'s graph in `src` and dump it to `path`, answering the
/// source-heap root. The heap stays the caller's, because comparing a hydrated
/// value against this root dereferences both sides.
fn dump_graph(src: &mut FiberHeap, path: &std::path::Path) -> Value {
    let region = src.new_runtime_region();
    let root = build_graph(src, region);
    elle::image::dump(src, &graph_names(), root, path).expect("dump");
    root
}

mod roundtrip {
    include!("image/roundtrip.rs");
}
mod mapping {
    include!("image/mapping.rs");
}
mod policy {
    include!("image/policy.rs");
}
mod containers {
    include!("image/containers.rs");
}
mod names {
    include!("image/names.rs");
}
mod source {
    include!("image/source.rs");
}
mod verify {
    include!("image/verify.rs");
}
