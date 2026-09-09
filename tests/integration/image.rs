// audited: 2026-09-08
// The image store milestone: dump a data-only value graph, hydrate it by
// private file mapping, and prove the mechanism end to end.
// docs/impl/image/plan.md
//
// An image is page bytes plus relocations, hydrated into an ordinary counted
// region. The submodules below hold the test plan's pins, and this file holds
// the graph every one of them dumps.

use elle::hir::region::RuntimeRegion;
use elle::value::fiberheap::FiberHeap;
use elle::value::{HeapObject, Pair, Value};

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

/// A representative data graph: nesting, every supported heap variant, and
/// supported immediates (ints, inline floats, bools, nil, keywords).
fn build_graph(heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
    let s = alloc_str(heap, region, "hello image");
    let b = alloc_bytes(heap, region, &[0xE1, 0x1E, 0x5C]);
    let inner = alloc_array(
        heap,
        region,
        &[Value::int(7), s, Value::keyword("spike"), Value::float(2.5)],
    );
    let tail = alloc_pair(heap, region, Value::bool(true), Value::EMPTY_LIST);
    let mid = alloc_pair(heap, region, inner, tail);
    let mid2 = alloc_pair(heap, region, b, mid);
    alloc_pair(heap, region, Value::int(1), mid2)
}

/// Build [`build_graph`]'s graph in `src` and dump it to `path`, answering the
/// source-heap root. The heap stays the caller's, because comparing a hydrated
/// value against this root dereferences both sides.
fn dump_graph(src: &mut FiberHeap, path: &std::path::Path) -> Value {
    let region = src.new_runtime_region();
    let root = build_graph(src, region);
    elle::image::dump(src, root, path).expect("dump");
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
mod source {
    include!("image/source.rs");
}
mod verify {
    include!("image/verify.rs");
}
