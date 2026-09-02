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
// (docs/impl/image.md risk item 6) keeps the files identical.
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

// ── The platform rule the file geometry is built on ─────────────────

// The trap, and the reason the pages section is aligned at all: `mmap`
// refuses a file offset that is not a multiple of the OS page size. This
// cost six hydration failures on macOS arm64, where the page is 16 KiB and
// the pages section started at a hardcoded 4 KiB — legal on the 4 KiB hosts
// every other CI job runs, EINVAL there.
//
// The counter-factual this pins: the round-trip tests above cannot catch
// that class of bug, because the dumper and the hydrator agreed on the wrong
// offset and agreement is all a round trip checks. This test asserts against
// the kernel instead of against the other half of our own code.
#[test]
fn mmap_refuses_a_file_offset_off_the_page_boundary() {
    let dir = crate::common::ScratchDir::new("image-mmap-offset");
    let path = dir.join("offsets.bin");

    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    std::fs::write(&path, vec![0u8; page * 4]).expect("write");
    let file = std::fs::File::open(&path).expect("open");
    let fd = std::os::fd::AsRawFd::as_raw_fd(&file);

    let map_at = |offset: usize| unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            offset as libc::off_t,
        )
    };

    // A page-aligned offset maps; half a page in does not.
    let good = map_at(page);
    assert_ne!(good, libc::MAP_FAILED, "page-aligned offset must map");
    unsafe { libc::munmap(good, page) };

    let bad = map_at(page / 2);
    assert_eq!(
        bad,
        libc::MAP_FAILED,
        "a half-page offset mapped; this platform's rule is not what the \
         image format assumes"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EINVAL),
        "expected EINVAL for a misaligned file offset"
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
    let hydrated = image::hydrate(&mut dst, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::keyword("just-me"));
}
