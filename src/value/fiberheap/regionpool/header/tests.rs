// audited: 2026-10-24
//! The page-base walk finds a page's true base past a size tag that mid-page bytes forge.
//!
//! docs/impl/region/generations.md
//! docs/impl/region/model.md

use super::*;
use crate::value::fiberheap::pagepool::{base_page, class_size_of, PagePool};
use crate::value::fiberheap::regionpool::{RegionPage, RegionPool};
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;

#[test]
fn header_lookup_rejects_mid_page_false_match() {
    // A pointer deep inside a LARGE page must resolve to the page's true base,
    // never to a smaller sub-alignment whose mid-page object data happens to
    // mimic a smaller page's size. The old header carried a bare `log2` byte, so
    // mid-page data could be read as a garbage header: `oracle.lisp`'s
    // `(first …)` result, deep in a 512 KiB page, masked down to an 8 KiB
    // sub-base whose byte 12 was 0x0D (= log2 8192) and read object data as a
    // header — region id 0xffffff45 — which then drove `ensure_raw` to a 584 GB
    // resize. The `size_tag` magic rejects the mid-page match.
    //
    // Counterfactual: pre-magic, `header_of_page_ptr` matched the forged
    // half-size sub-base (its byte 12 equals that size's log2) and returned the
    // forged region id; post-magic the forged tag lacks the magic, so the walk
    // reaches the true base and returns region 42.
    let mut pool = PagePool::new(base_page(), 8 * 1024 * 1024);
    // A class-2 page, self-aligned by `claim`.
    let big = class_size_of(base_page(), 2);
    let page = pool.claim(big);
    assert_eq!(page.len(), big);
    let base = page.as_ptr() as usize;
    // Stamp the real header at the true base (region 42).
    let _rp = RegionPage::new(
        page,
        42,
        PageStamp {
            generation: 3,
            store: 7,
        },
    );
    // Forge a "smaller page" header mid-page, at the class-1-aligned sub-base:
    // a garbage region id, and a bare `log2(half)` at the `size_tag` offset
    // (12) WITHOUT the magic — exactly the mid-page object data that fooled the
    // old single-byte check.
    let half = class_size_of(base_page(), 1);
    let sub_base = base + half; // half a page in — half-page-aligned
    unsafe {
        *(sub_base as *mut u32) = 0xDEAD_BEEF; // forged region_id
        *((sub_base + 12) as *mut u32) = half.trailing_zeros(); // bare log2, no magic
    }
    // A pointer deep in the page, past the forged sub-base.
    let deep = (sub_base + 0x620) as *const ();
    let rid = unsafe { region_of_page_ptr(deep, base_page()) };
    assert_eq!(
        rid, 42,
        "region_of_page_ptr resolved a deep pointer to a mid-page false header \
         (got 0x{rid:x}) instead of the page's true base region 42 — the size_tag \
         magic must reject a sub-aligned mid-page match"
    );
}

#[test]
fn a_slice_header_ends_in_a_word_no_size_tag_can_match() {
    // A `RegionSlice` is sixteen bytes and its last four sit exactly where a
    // page header's `size_tag` is read, so those four have to be written rather
    // than left as padding (docs/impl/region/generations.md). Zero is the
    // value: a `size_tag` always carries the magic in its high 24 bits.
    //
    // The write goes into a poisoned cell, so what the assertion reads is the
    // slice header's own bytes and not whatever the cell happened to hold.
    let payload = [0xABu8; 4];
    let mut cell = [0xE1u8; 16];
    unsafe {
        (cell.as_mut_ptr() as *mut RegionSlice<u8>)
            .write(RegionSlice::from_raw(payload.as_ptr(), 4))
    };
    assert_eq!(
        &cell[12..16],
        &[0, 0, 0, 0],
        "a slice header left its last four bytes unwritten, so it publishes \
         whatever the page held there — and a recycled page can hold a real \
         size_tag",
    );
}

#[test]
fn a_slice_header_written_mid_page_cannot_forge_a_page_base() {
    // The trap: a claimed page's body is whatever its last occupant left
    // (docs/impl/region/model.md), so a `base_page`-aligned
    // address inside a LARGE page can already hold a word that reads as a base
    // page's `size_tag`. A slice header landing on that address has to clear
    // it, because the page-base walk masks a pointer to the smallest candidate
    // first and stops at the first header that self-validates.
    //
    // The counter-factual is a slice header that leaves those four bytes alone:
    // the walk then stops at the forged base, hands back the garbage id it
    // reads there, and the free-time cross-ref scan drops the cross-region edge
    // its ownership filter rejects — a leak in release, and edge-table drift at
    // the next free in debug.
    let mut pool = PagePool::new(base_page(), 8 * 1024 * 1024);

    // A genuine base-page `size_tag`, read off a page the pool really stamped.
    // Forging one from the magic would restate a constant the header owns.
    let probe = pool.claim(base_page());
    let probe_base = probe.as_ptr() as usize;
    let _probe = RegionPage::new(probe, 7, PageStamp::default());
    let base_tag = unsafe { *((probe_base + 12) as *const u32) };

    let big = class_size_of(base_page(), 2);
    let page = pool.claim(big);
    let base = page.as_ptr() as usize;
    let _held = RegionPage::new(
        page,
        42,
        PageStamp {
            generation: 3,
            store: 7,
        },
    );

    // The stale word one base page in, as a recycled page would carry it.
    let sub_base = base + base_page();
    unsafe { *((sub_base + 12) as *mut u32) = base_tag };
    assert!(
        unsafe { header_if_valid(sub_base, base_page()) }.is_some(),
        "the forged base must self-validate, or this test proves nothing",
    );

    // The region writes a slice header over it.
    let payload = [0xABu8; 8];
    unsafe { (sub_base as *mut RegionSlice<u8>).write(RegionSlice::from_raw(payload.as_ptr(), 8)) };

    let deep = (sub_base + 0x40) as *const ();
    let rid = unsafe { region_of_page_ptr(deep, base_page()) };
    assert_eq!(
        rid, 42,
        "a pointer past a slice header resolved to a forged mid-page base \
         (got 0x{rid:x}) instead of region 42 — the slice header left the four \
         bytes a size_tag occupies standing",
    );
}

#[test]
fn no_object_slot_covers_a_header_window_inside_its_page() {
    // A `HeapObject` slot is a whole enum, so a short variant leaves most of
    // its slot as the construction temporary held it, and memcheck caught the
    // walk reading such a byte (docs/impl/region/generations.md). The first
    // `HEADER_SIZE` bytes at each `base_page` boundary inside a page are where
    // the walk reads a size tag, so no slot may cover them.
    //
    // The page is a recycled one whose every boundary holds a genuine base-page
    // size tag, so bytes the allocator passed without writing would still read
    // as a page base. The counter-factual is the plain bump: its slots cross
    // every boundary, and the first assertion fails at the first one.
    let mut pool = PagePool::new(base_page(), 8 * 1024 * 1024);
    let big = class_size_of(base_page(), 2);
    let trace = || std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

    let probe = pool.claim(base_page());
    let probe_base = probe.as_ptr() as usize;
    let _probe = RegionPage::new(probe, 7, PageStamp::default());
    let base_tag = unsafe { *((probe_base + 12) as *const u32) };

    let mut dead = RegionPool::new(41, PageStamp::default(), big, trace());
    dead.alloc_region_slice(b"x", &mut pool);
    let base = dead.pages[0].page.as_ptr() as usize;
    for boundary in (base + base_page()..base + big).step_by(base_page()) {
        unsafe { *((boundary + 12) as *mut u32) = base_tag };
    }
    dead.teardown(&mut pool);

    let mut live = RegionPool::new(42, PageStamp::default(), big, trace());
    let mut slots = Vec::new();
    while live.pages.len() < 2 {
        let v = live.alloc_obj(HeapObject::LibHandle(7), &mut pool);
        slots.push(v.as_heap_ptr().unwrap() as usize);
    }
    let page = &live.pages[0];
    assert_eq!(
        page.page.as_ptr() as usize,
        base,
        "the region must claim the recycled page, or this test proves nothing",
    );
    let slots: Vec<usize> = slots.into_iter().filter(|&s| s < base + big).collect();
    let size = std::mem::size_of::<HeapObject>();
    for boundary in (base + base_page()..base + page.obj_cursor).step_by(base_page()) {
        for &slot in &slots {
            assert!(
                slot + size <= boundary || slot >= boundary + HEADER_SIZE,
                "the slot at {:#x} covers the header bytes at {:#x}",
                slot - base,
                boundary - base,
            );
        }
        let window = unsafe { std::slice::from_raw_parts(boundary as *const u8, HEADER_SIZE) };
        assert_eq!(
            window,
            &[0u8; HEADER_SIZE][..],
            "the header bytes at {:#x} below the object cursor are not zero",
            boundary - base,
        );
    }
    for &slot in &slots {
        let rid = unsafe { region_of_page_ptr(slot as *const (), base_page()) };
        assert_eq!(
            rid,
            42,
            "the walk from the slot at {:#x} found a forged base",
            slot - base
        );
    }
}
