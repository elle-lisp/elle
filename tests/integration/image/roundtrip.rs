// audited: 2026-09-08
// What comes back out of an image: the same graph, the same sharing, the same
// bytes on a second dump.
// docs/impl/image/plan.md

use super::*;
use elle::image::{self, ImageError};
use elle::value::heap::deref;

// § Test plan, "Round-trip": dump a data graph, hydrate in a fresh heap,
// assert structural equality. The fresh heap is what a fresh process would
// hold: nothing in it predates the hydration, so equality can only come from
// the mapped pages and the relocation pass.
#[test]
fn data_graph_round_trips_through_dump_and_hydrate() {
    let dir = crate::common::ScratchDir::new("image-roundtrip");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let root = dump_graph(&mut src, &path);

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
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
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
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

// An immediate root needs no pages at all; the round trip is the header.
#[test]
fn immediate_root_round_trips() {
    let dir = crate::common::ScratchDir::new("image-immediate");
    let path = dir.join("imm.image");

    let mut src = FiberHeap::new();
    image::dump(&mut src, Value::keyword("just-me"), &path).expect("dump");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::keyword("just-me"));
}

// ── Fingerprint fallback ────────────────────────────────────────────

// § Test plan, "Round-trip": a load with a corrupted fingerprint falls back
// cleanly — a typed error, no region minted, no mapping left behind.
#[test]
fn corrupted_fingerprint_falls_back_cleanly() {
    let dir = crate::common::ScratchDir::new("image-fingerprint");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    dump_graph(&mut src, &path);

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
    match image::hydrate_path(&mut dst, &path) {
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
    match image::hydrate_path(&mut dst, &path) {
        Err(ImageError::Corrupt(_)) => {}
        other => panic!("expected corrupt-image error, got {other:?}"),
    }
}

// § Fingerprint: the layout probes participate in the fingerprint, so a
// binary whose `HeapObject` layout shifted rejects the image instead of
// hydrating garbage. Pin that every dumpable variant appears with extents.
#[test]
fn fingerprint_records_variant_layouts() {
    let fp = image::fingerprint();
    assert!(fp.contains("layout="), "no layout section: {fp}");
    for variant in ["LString", "Pair", "LArray", "LBytes", "Float"] {
        assert!(fp.contains(variant), "layout section lacks {variant}: {fp}");
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
    let root = dump_graph(&mut src, &path);

    let mut dst = FiberHeap::new();
    let first = image::hydrate_path(&mut dst, &path).expect("first hydrate");
    let second = image::hydrate_path(&mut dst, &path).expect("second hydrate");
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

/// Fill `depth + 1` stack frames with `pattern` so that any construction
/// temporary a later call materializes inherits pattern bytes in its
/// padding. The xor keeps the recursion and the buffer observable.
#[inline(never)]
fn paint_stack(pattern: u8, depth: usize) -> u64 {
    let buf = [pattern; 4096];
    let sum: u64 = buf.iter().map(|&b| b as u64).sum();
    if depth == 0 {
        sum
    } else {
        sum ^ paint_stack(pattern, depth - 1)
    }
}

// § Test plan, "Determinism" / § Dumping: two dumps of the same graph are
// byte-identical whole files. The counter-factual is the stack painting:
// the dumper's scratch objects are `repr(Rust)` enum copies whose padding
// comes from their construction temporaries, so a dumper that copies slot
// bytes wholesale writes whatever the stack held into the file — painting
// the stack differently before each dump forced ~700 differing bytes.
// Only a dumper that assembles slots from the probed field extents
// (docs/impl/image/measurements.md item 6) keeps the files identical.
#[test]
fn dump_is_byte_deterministic_whole_file() {
    let dir = crate::common::ScratchDir::new("image-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    paint_stack(0xAA, 16);
    image::dump(&mut src, root, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(&mut src, root, &b).expect("dump b");

    let ba = std::fs::read(&a).expect("read a");
    let bb = std::fs::read(&b).expect("read b");
    let diff: Vec<usize> = (0..ba.len().min(bb.len()))
        .filter(|&i| ba[i] != bb[i])
        .collect();
    assert_eq!(ba.len(), bb.len(), "dump lengths differ");
    assert!(
        diff.is_empty(),
        "dumps differ at {} offsets, first: {:?}",
        diff.len(),
        &diff[..diff.len().min(16)]
    );
}
