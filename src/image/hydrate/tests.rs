// audited: 2026-10-12
//! The interval table: a hydrated region's pointers resolve, and its pool answers ownership, without reading a page header.
//!
//! docs/impl/image.md
//! docs/impl/region/generations.md

use std::path::Path;

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::regionpool::{header_if_valid, PageLayout, HEADER_SIZE};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{HeapObject, Pair};
use crate::value::Value;

use super::super::Hydrated;

/// Pairs in the dumped list: enough that the dumper's scratch region grows
/// through several page sizes, so the image holds pages above the base page.
const PAIRS: i64 = 4000;

/// Dump a list of [`PAIRS`] integers to `path`, built in a heap of its own.
fn dump_list(path: &Path) {
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let mut list = Value::EMPTY_LIST;
    for i in 0..PAIRS {
        list = src.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(i), list)), region);
    }
    super::super::dump(&mut src, &SymbolTable::new(), list, path).expect("dump");
}

/// Hydrate `path` into `heap`.
fn hydrate(heap: &mut FiberHeap, path: &Path) -> Hydrated {
    super::hydrate_path(heap, &mut SymbolTable::new(), path).expect("hydrate")
}

/// The page layouts of a hydrated region.
fn pages_of(heap: &FiberHeap, region: RuntimeRegion) -> Vec<PageLayout> {
    heap.region_pool(region)
        .expect("the hydrated region is live")
        .page_layouts()
}

/// Every page header of a region with its region id overwritten by zero,
/// which names no region. Restored when this goes out of scope, before the
/// heap's teardown or a free reads a header again.
struct Overwritten {
    saved: Vec<(usize, u32)>,
}

impl Overwritten {
    fn ids(pages: &[PageLayout]) -> Self {
        let mut saved = Vec::new();
        for p in pages {
            // SAFETY: a hydrated page is a private writable mapping, and the
            // region id is the header's first field.
            unsafe {
                let id = p.base as *mut u32;
                saved.push((p.base, *id));
                *id = 0;
            }
            // The header must still validate, so a resolver that reads it
            // stops there and reads the zero.
            let header = unsafe { header_if_valid(p.base, p.len) };
            assert_eq!(
                header.map(|(id, _)| id),
                Some(0),
                "the overwrite broke the header at {:#x}, so the probe ladder \
                 would walk past it",
                p.base
            );
        }
        Overwritten { saved }
    }
}

impl Drop for Overwritten {
    fn drop(&mut self) {
        for &(base, id) in &self.saved {
            unsafe { *(base as *mut u32) = id };
        }
    }
}

/// Three addresses inside a page: its first object slot, its middle, and its
/// last byte.
fn inside(p: &PageLayout) -> [usize; 3] {
    [p.base + HEADER_SIZE, p.base + p.len / 2, p.base + p.len - 1]
}

// The trap: the overwrite keeps the size tag, so the header still validates.
// A broken tag would send the ladder past the page base, into memory below
// the image that nothing mapped.
//
// The counter-factual is the probe ladder, which stops at the first header
// that validates and answers the zero it finds there: no region. Two images in
// one heap are the table's usual two entries, and each page must answer its
// own region rather than the first image's.
#[test]
fn a_hydrated_pointer_resolves_with_every_header_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let first = hydrate(&mut heap, &path);
    let second = hydrate(&mut heap, &path);
    for image in [first, second] {
        let pages = pages_of(&heap, image.region);
        assert!(
            pages.len() >= 3 && pages.iter().any(|p| p.len > heap.region_page_size()),
            "the image must hold several pages, some above the base page"
        );
        let _overwritten = Overwritten::ids(&pages);
        for p in &pages {
            for addr in inside(p) {
                assert_eq!(
                    heap.region_of_ptr(addr as *const ()),
                    image.region.get(),
                    "{addr:#x} in the {}-byte page at {:#x} resolved through its \
                     header",
                    p.len,
                    p.base
                );
            }
        }
    }
}

// The allocation funnel resolves every value an object stores, to record the
// edge that keeps the value's region alive. It takes the store's resolver,
// not `region_of_ptr`, so it needs a pin of its own.
//
// The counter-factual is the probe ladder again: it answers no region, so the
// funnel records no edge, and the holder outlives nothing it points into.
#[test]
fn an_allocation_records_its_edge_into_an_image_with_every_header_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let image = hydrate(&mut heap, &path);
    let holder = heap.new_runtime_region();
    {
        let _overwritten = Overwritten::ids(&pages_of(&heap, image.region));
        heap.alloc_in_region(
            HeapObject::Pair(Pair::new(image.root, Value::EMPTY_LIST)),
            holder,
        );
    }
    assert_eq!(
        heap.outgoing_edges(holder),
        vec![(image.region.get(), 1)],
        "the holder recorded no edge into the image"
    );
    assert_eq!(heap.region_rc(image.region), 2);

    heap.decref_region_if_present(holder);
    assert_eq!(
        heap.region_rc(image.region),
        1,
        "freeing the holder did not release its edge into the image"
    );
}

// The interval is the pool's answer to `owns`, so it must be exactly the
// pages: a byte beyond either end is another mapping's.
#[test]
fn a_hydrated_pool_answers_its_interval_and_owns_exactly_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let image = hydrate(&mut heap, &path);
    let pages = pages_of(&heap, image.region);
    let start = pages.iter().map(|p| p.base).min().expect("pages");
    let end = pages.iter().map(|p| p.base + p.len).max().expect("pages");
    assert_eq!(
        end - start,
        pages.iter().map(|p| p.len).sum::<usize>(),
        "the hydrated pages leave a gap"
    );

    let pool = heap.region_pool(image.region).expect("live");
    assert_eq!(pool.image_interval(), Some(start..end));
    assert!(pool.owns(start as *const ()), "the first byte");
    assert!(pool.owns((end - 1) as *const ()), "the last byte");
    assert!(!pool.owns((start - 1) as *const ()), "the byte below");
    assert!(!pool.owns(end as *const ()), "the byte above");
}

// A hydrated region is an ordinary counted region, so it can claim a page
// after hydration. That page lies outside the interval, and only the page
// list and its header can answer for it.
#[test]
fn a_page_claimed_after_hydration_resolves_outside_the_interval() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let image = hydrate(&mut heap, &path);
    let largest = pages_of(&heap, image.region)
        .iter()
        .map(|p| p.len)
        .max()
        .expect("pages");
    let bytes = heap.alloc_bytes_in_region_with(largest, image.region, |b| b.fill(7));
    let addr = bytes.as_ptr() as usize;

    let pool = heap.region_pool(image.region).expect("live");
    let interval = pool.image_interval().expect("a hydrated pool has one");
    assert!(
        !interval.contains(&addr),
        "the bytes landed inside the image"
    );
    assert!(pool.owns(addr as *const ()));
    assert_eq!(heap.region_of_ptr(addr as *const ()), image.region.get());
}

// The table is read on every resolution, so a region that stayed in it after
// its pages went would cost every later lookup, and both paths that return a
// region's pages must take it out: the free at rc zero and the wholesale
// teardown.
#[test]
fn a_hydrated_region_leaves_the_interval_table_with_its_pages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("list.image");
    dump_list(&path);

    let mut heap = FiberHeap::new();
    let freed = hydrate(&mut heap, &path);
    let torn = hydrate(&mut heap, &path);
    assert_eq!(heap.interval_table(), vec![freed.region, torn.region]);

    heap.decref_region_if_present(freed.region);
    assert_eq!(heap.interval_table(), vec![torn.region]);

    heap.clear();
    assert_eq!(heap.interval_table(), Vec::<RuntimeRegion>::new());
}
