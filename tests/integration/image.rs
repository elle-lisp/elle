// The store spike (docs/impl/image.md § "Landing order" item 5, dispatched as
// risk-item 3): dump a data-only value graph as page bytes plus relocations,
// hydrate it by private file mapping into an ordinary counted region, and
// prove the format, the mapping, the relocation pass, and teardown end to
// end. These are the § "Test plan" pins the store milestone must keep.

use std::rc::Rc;

use elle::image::{self, ImageError};
use elle::value::fiberheap::FiberHeap;
use elle::value::heap::deref;
use elle::value::{HeapObject, Pair, Value};

// ── Graph builders (trait-less data values, one region) ─────────────

fn alloc_str(heap: &mut FiberHeap, region: elle::hir::region::RuntimeRegion, s: &str) -> Value {
    let slice = heap.alloc_region_slice_in_region(s.as_bytes(), region);
    heap.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: Value::NIL,
        },
        region,
    )
}

fn alloc_bytes(heap: &mut FiberHeap, region: elle::hir::region::RuntimeRegion, b: &[u8]) -> Value {
    let slice = heap.alloc_region_slice_in_region(b, region);
    heap.alloc_in_region(
        HeapObject::LBytes {
            data: slice,
            traits: Value::NIL,
        },
        region,
    )
}

fn alloc_pair(
    heap: &mut FiberHeap,
    region: elle::hir::region::RuntimeRegion,
    first: Value,
    rest: Value,
) -> Value {
    heap.alloc_in_region(HeapObject::Pair(Pair::new(first, rest)), region)
}

fn alloc_array(
    heap: &mut FiberHeap,
    region: elle::hir::region::RuntimeRegion,
    items: &[Value],
) -> Value {
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
fn build_graph(heap: &mut FiberHeap, region: elle::hir::region::RuntimeRegion) -> Value {
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

// ── Round-trip ──────────────────────────────────────────────────────

// § Test plan, "Round-trip": dump a data graph, hydrate in a fresh heap,
// assert structural equality. The fresh heap is what a fresh process would
// hold: nothing in it predates the hydration, so equality can only come from
// the mapped pages and the relocation pass.
#[test]
fn data_graph_round_trips_through_dump_and_hydrate() {
    let dir = crate::common::ScratchDir::new("image-roundtrip");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    assert_eq!(root, hydrated.root, "hydrated graph differs from source");
}

// Sharing is preserved (§ Dumping: the visited map keys on payload address,
// "cycles and sharing preserved — unlike `send`"). The counter-factual: a
// per-edge copier would round-trip to a structurally equal graph and pass
// the test above while silently doubling every shared subgraph.
#[test]
fn hydration_preserves_sharing() {
    let dir = crate::common::ScratchDir::new("image-sharing");
    let path = dir.join("shared.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let shared = alloc_str(&mut src, region, "shared once");
    let root = alloc_array(&mut src, region, &[shared, shared]);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    let obj = unsafe { deref(hydrated.root) };
    let HeapObject::LArray { elements, .. } = obj else {
        panic!("hydrated root is not an array");
    };
    let elems = elements.as_slice();
    assert_eq!(elems.len(), 2);
    assert_eq!(
        elems[0].as_heap_ptr(),
        elems[1].as_heap_ptr(),
        "shared child was duplicated by the dump"
    );
}

// ── Fingerprint fallback ────────────────────────────────────────────

// § Test plan, "Round-trip": a load with a corrupted fingerprint falls back
// cleanly — a typed error, no region minted, no mapping left behind.
#[test]
fn corrupted_fingerprint_falls_back_cleanly() {
    let dir = crate::common::ScratchDir::new("image-fingerprint");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    // Corrupt one byte inside the stored fingerprint string.
    let mut bytes = std::fs::read(&path).expect("read image");
    let fp = image::fingerprint();
    let pos = bytes
        .windows(fp.len())
        .position(|w| w == fp.as_bytes())
        .expect("fingerprint not found in image header");
    bytes[pos] ^= 0x20;
    std::fs::write(&path, &bytes).expect("rewrite image");

    let mut dst = FiberHeap::new();
    let before = dst.active_region_count();
    match image::hydrate(&mut dst, &path) {
        Err(ImageError::Fingerprint { .. }) => {}
        other => panic!("expected fingerprint mismatch, got {other:?}"),
    }
    assert_eq!(
        dst.active_region_count(),
        before,
        "failed hydration leaked a region"
    );
}

// A file that is not an image at all is rejected as corrupt, not mapped.
#[test]
fn garbage_file_is_rejected() {
    let dir = crate::common::ScratchDir::new("image-garbage");
    let path = dir.join("garbage.image");
    std::fs::write(&path, b"not an image at all").expect("write");
    let mut dst = FiberHeap::new();
    match image::hydrate(&mut dst, &path) {
        Err(ImageError::Corrupt(_)) => {}
        other => panic!("expected corrupt-image error, got {other:?}"),
    }
}

// ── Relocation independence ─────────────────────────────────────────

// § Test plan, "Relocation": hydrate the same image twice in one process —
// two regions, two address sets — and assert both hydrations are correct
// and independent (freeing the first leaves the second intact).
#[test]
fn double_hydration_is_correct_and_independent() {
    let dir = crate::common::ScratchDir::new("image-double");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let first = image::hydrate(&mut dst, &path).expect("first hydrate");
    let second = image::hydrate(&mut dst, &path).expect("second hydrate");
    assert_ne!(
        first.root.as_heap_ptr(),
        second.root.as_heap_ptr(),
        "two hydrations share an address set"
    );
    assert_eq!(root, first.root);
    assert_eq!(root, second.root);

    dst.decref_region_if_present(first.region);
    assert_eq!(
        root, second.root,
        "freeing the first hydration corrupted the second"
    );
}

// ── Determinism ─────────────────────────────────────────────────────

// § Test plan, "Determinism" / § Dumping: the ordered walk makes the dump's
// LAYOUT deterministic — identical lengths, identical header and metadata
// sections — and any byte difference between two dumps of the same graph is
// confined to object slots. The trap the spike measured: a `repr(Rust)`
// enum copy carries uninitialized padding from its construction temporary,
// so raw object-slot bytes cannot be canonicalized until the field-extent
// probes (risk item 6) land — but headers, gaps, alignment slack, and
// relocation slots ARE canonicalized (zeroed), and this test fails if any
// of those, or any metadata byte, ever differs.
#[test]
fn dump_layout_is_deterministic_and_diffs_confine_to_object_slots() {
    let dir = crate::common::ScratchDir::new("image-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &a).expect("dump a");
    image::dump(&mut src, root, &b).expect("dump b");

    let ba = std::fs::read(&a).expect("read a");
    let bb = std::fs::read(&b).expect("read b");
    assert_eq!(ba.len(), bb.len(), "dump lengths differ");

    // Header geometry (fixed little-endian offsets; see image/format.rs).
    let u64_at = |buf: &[u8], at: usize| u64::from_le_bytes(buf[at..at + 8].try_into().unwrap());
    const HEADER_BLOCK: usize = 4096;
    let pages_len = u64_at(&ba, 16) as usize;
    let n_pages = u64_at(&ba, 24) as usize;

    assert_eq!(ba[..HEADER_BLOCK], bb[..HEADER_BLOCK], "headers differ");
    let meta_at = HEADER_BLOCK + pages_len;
    assert_eq!(ba[meta_at..], bb[meta_at..], "metadata sections differ");

    // Object spans per page, from the page table: [16, obj_cursor) at each
    // page's placement offset. Every remaining diff must fall inside one.
    let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
    let mut page_off = HEADER_BLOCK;
    for i in 0..n_pages {
        let entry = meta_at + i * 24;
        let size = u64_at(&ba, entry) as usize;
        let obj_cursor = u64_at(&ba, entry + 8) as usize;
        spans.push(page_off + 16..page_off + obj_cursor);
        page_off += size;
    }
    let stray: Vec<usize> = (HEADER_BLOCK..meta_at)
        .filter(|&i| ba[i] != bb[i] && !spans.iter().any(|s| s.contains(&i)))
        .collect();
    assert!(
        stray.is_empty(),
        "dumps differ outside object slots at offsets {:?}",
        &stray[..stray.len().min(16)]
    );
}

// ── Pool interplay ──────────────────────────────────────────────────

// § Test plan, "Mapping": release a file-backed page and assert the pool
// unmapped it rather than caching it. Per-heap byte accounting is the
// race-free observable: `allocated_bytes` counts region pages AND cached
// pages, so if the freed hydrated pages were cached the count would not
// return to its pre-hydration value.
#[test]
fn hydrated_pages_release_by_munmap_not_cache() {
    let dir = crate::common::ScratchDir::new("image-munmap");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let before = dst.allocated_bytes();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    assert!(
        dst.allocated_bytes() > before,
        "hydration added no page bytes"
    );
    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.allocated_bytes(),
        before,
        "freed hydrated pages were cached instead of unmapped"
    );
}

// § Test plan, "Hygiene": hydrate, free, and the heap returns to its
// baseline — no image-specific carve-out in the region accounting.
#[test]
fn hydration_teardown_returns_to_baseline() {
    let dir = crate::common::ScratchDir::new("image-baseline");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");
    // The dump's scratch region was dropped: the source heap is back to its
    // own baseline (the graph's one region).
    assert_eq!(src.active_region_count(), 1, "dump leaked its scratch");

    let mut dst = FiberHeap::new();
    let regions_before = dst.region_info_vec();
    let objs_before = dst.visible_len();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    assert!(dst.visible_len() > objs_before);
    dst.decref_region_if_present(hydrated.region);
    assert_eq!(dst.region_info_vec(), regions_before);
    assert_eq!(dst.visible_len(), objs_before);
}

// ── Mapping stability ───────────────────────────────────────────────

// § Hydration: "Never rewrite an image file in place" — the atomic
// temp-and-rename discipline keeps a mapped old inode stable while a new
// file replaces the path. Pin the read side: replace the file by rename
// while a hydration is live, then read the hydrated values.
#[test]
fn replaced_file_keeps_live_mapping_intact() {
    let dir = crate::common::ScratchDir::new("image-rename");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");

    // Replace the path with a different (garbage) file via rename.
    let replacement = dir.join("replacement");
    std::fs::write(&replacement, vec![0xAA; 1 << 16]).expect("write replacement");
    std::fs::rename(&replacement, &path).expect("rename over image");

    assert_eq!(
        root, hydrated.root,
        "live mapping read through the replaced path's old inode failed"
    );
}

// ── Dump policy ─────────────────────────────────────────────────────

// § Dumping: "Unsupported values fail the dump with an error naming the
// binding" — for the spike, naming the refused variant. A mutable store
// must never be silently dropped or frozen into the body.
#[test]
fn mutable_value_refuses_dump() {
    let dir = crate::common::ScratchDir::new("image-mutable");
    let path = dir.join("mutable.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arr = src.alloc_in_region(
        HeapObject::LArrayMut {
            data: Rc::new(std::cell::RefCell::new(vec![Value::int(1)])),
            traits: Value::NIL,
        },
        region,
    );
    let root = alloc_pair(&mut src, region, Value::int(1), arr);
    match image::dump(&mut src, root, &path) {
        Err(ImageError::Unsupported(what)) => {
            assert!(
                what.contains("LArrayMut"),
                "refusal does not name the variant: {what}"
            );
        }
        other => panic!("expected unsupported-value refusal, got {other:?}"),
    }
    assert!(!path.exists(), "refused dump left a partial file");
}

// A carried traitset is a reference out of the data graph (the default
// traitsets are instance infrastructure — § "Process-owned resources
// reconstruct in place"); the spike's dumper refuses it rather than
// persisting a dangling instance pointer.
#[test]
fn traited_value_refuses_dump() {
    let dir = crate::common::ScratchDir::new("image-traits");
    let path = dir.join("traits.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let table = alloc_str(&mut src, region, "pretend traitset");
    let slice = src.alloc_region_slice_in_region("x".as_bytes(), region);
    let traited = src.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: table,
        },
        region,
    );
    match image::dump(&mut src, traited, &path) {
        Err(ImageError::Unsupported(what)) => {
            assert!(what.contains("traits"), "refusal does not say traits: {what}");
        }
        other => panic!("expected traits refusal, got {other:?}"),
    }
}

// An immediate root needs no pages at all; the round trip is the header.
#[test]
fn immediate_root_round_trips() {
    let dir = crate::common::ScratchDir::new("image-immediate");
    let path = dir.join("imm.image");

    let mut src = FiberHeap::new();
    image::dump(&mut src, Value::keyword("just-me"), &path).expect("dump");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::keyword("just-me"));
}
