// audited: 2026-09-08
//! A hydrated region under `--trace=scrub`: the diagnostic leaves the image's
//! values alone, and parks none of its pages in the cache.
//!
//! docs/impl/image/plan.md
//!
//! Scrub blanks the spans a dying region wrote before its page goes back to
//! the pool, so a read through a pointer that outlived its region lands on
//! zeros. An image page is not a pool page: its bytes are a private view of a
//! file, and its release is `munmap`. The two rules meet at
//! `PagePool::release`, and this file is the pin that they meet in the right
//! order.
//!
//! This file is its OWN test binary because `config::init` is process-global:
//! arming scrub inside the shared integration binary would blank pages for
//! every other test in it.

use elle::image;
use elle::value::fiberheap::FiberHeap;
use elle::value::{HeapObject, Pair, Value};

/// The graph the store milestone's tests dump: nesting, a string, bytes, an
/// array, and the portable immediates.
fn build_graph(heap: &mut FiberHeap, region: elle::hir::region::RuntimeRegion) -> Value {
    let bytes = heap.alloc_region_slice_in_region("hello image".as_bytes(), region);
    let s = heap.alloc_in_region(
        HeapObject::LString {
            s: bytes,
            traits: Value::NIL,
        },
        region,
    );
    let items = heap.alloc_region_slice_in_region(
        &[Value::int(7), s, Value::keyword("scrub"), Value::float(2.5)],
        region,
    );
    let arr = heap.alloc_in_region(
        HeapObject::LArray {
            elements: items,
            traits: Value::NIL,
        },
        region,
    );
    heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), arr)), region)
}

// § Test plan, "Diagnostics": under `--trace=scrub` a hydrated image answers
// exactly as it does without the flag, and its pages are unmapped on release
// rather than blanked into the cache.
//
// The counter-factual is the order of the two checks in `PagePool::release`.
// A release that scrubbed before noticing the page is file-backed would write
// zeros through a `MAP_PRIVATE` view — costing a copy-on-write fault per
// frame — and then cache the page, so the next claim would hand a region a
// page of image bytes. The byte count is what sees that: it counts region
// pages and cached pages alike, so a cached page keeps it above the baseline.
#[test]
fn a_hydrated_region_survives_scrub_and_unmaps_on_release() {
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("scrub".to_string());
    elle::config::init(cfg);

    let dir = std::env::temp_dir().join(format!("elle-image-scrub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);
    image::dump(&mut src, root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let baseline = dst.allocated_bytes();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
    assert_eq!(
        root, hydrated.root,
        "the hydrated graph differs from its source under --trace=scrub"
    );

    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.allocated_bytes(),
        baseline,
        "a scrubbed release cached the image's pages instead of unmapping them"
    );

    std::fs::remove_dir_all(&dir).ok();
}
