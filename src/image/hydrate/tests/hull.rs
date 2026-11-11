// audited: 2026-11-11
//! The interval table's hull: a pointer outside every image skips the table, and the hull follows the entries.
//!
//! docs/impl/image/hydration.md

use super::*;
use crate::value::fiberheap::regionstore::RegionStore;

/// The interval the pages of the region `image` hydrated into tile.
fn interval_of(heap: &FiberHeap, image: RuntimeRegion) -> std::ops::Range<usize> {
    heap.region_pool(image)
        .expect("the hydrated region is live")
        .image_interval()
        .expect("a hydrated pool has one")
}

// Every reference count a running program moves resolves a pointer, and most
// of those pointers lie in pages claimed after boot. The counter-factual is the
// scan the hull stands in front of: it reads every entry before the ladder, so
// such a pointer pays one probe per image.
#[test]
fn a_pointer_outside_every_image_scans_no_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let image = hydrate(&mut heap, &path);
    let holder = heap.new_runtime_region();
    let pair = heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(1), Value::EMPTY_LIST)),
        holder,
    );
    let ptr = pair.as_heap_ptr().expect("a heap pair");

    let before = RegionStore::image_probes();
    assert_eq!(heap.region_of_ptr(ptr), holder.get());
    assert_eq!(
        RegionStore::image_probes() - before,
        0,
        "a pointer no image holds scanned the interval table"
    );
    let root = image.root.as_heap_ptr().expect("a heap root");
    assert_eq!(heap.region_of_ptr(root), image.region.get());
    heap.decref_region_if_present(holder);
}

// The hull is the table's to keep: it grows with each hydration and is
// recomputed from the entries left when a region's pages go. A hull that kept
// a released image's span would send a pointer that lands there through the
// scan for nothing, and one that lost a live image's span would send an image
// pointer down the ladder.
#[test]
fn the_hull_covers_every_image_and_shrinks_as_one_leaves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let first = hydrate(&mut heap, &path).region;
    let second = hydrate(&mut heap, &path).region;
    let (a, b) = (interval_of(&heap, first), interval_of(&heap, second));
    assert_eq!(
        heap.interval_hull(),
        Some(a.start.min(b.start)..a.end.max(b.end))
    );

    heap.decref_region_if_present(first);
    assert_eq!(heap.interval_hull(), Some(b));

    heap.clear();
    assert_eq!(heap.interval_hull(), None);
}
