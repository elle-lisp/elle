//! Unit tests (`super` is the parent impl module).

use super::super::pagepool::BASE_PAGE;
use super::*;
use crate::value::heap::Pair;

/// A pool for `region_id` with a default (generation 0, store 0) stamp.
fn pool_for(region_id: u32) -> RegionPool {
    RegionPool::new(region_id, PageStamp::default(), BASE_PAGE)
}

fn cons_obj() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
}

#[test]
fn alloc_obj_basic() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    assert!(v.is_heap());
    assert_eq!(rp.obj_count(), 1);
}

#[test]
fn alloc_multiple_objects() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    let mut ptrs = vec![];
    for _ in 0..50 {
        let v = rp.alloc_obj(cons_obj(), &mut pool);
        ptrs.push(v.as_heap_ptr().unwrap() as usize);
    }
    assert_eq!(rp.obj_count(), 50);
    // All pointers should be unique.
    let unique: std::collections::HashSet<_> = ptrs.iter().collect();
    assert_eq!(unique.len(), 50);
}

#[test]
fn alloc_region_slice_basic() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    let s = rp.alloc_region_slice(b"hello world", &mut pool);
    assert_eq!(s.as_slice(), b"hello world");
}

#[test]
fn dual_ended_layout() {
    // Objects and data should coexist on the same page.
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);

    // Allocate some inline data first.
    let s = rp.alloc_region_slice(b"test data", &mut pool);
    // Then an object.
    let v = rp.alloc_obj(cons_obj(), &mut pool);

    // Both should be on the same page (single 4K page).
    assert_eq!(rp.pages.len(), 1);
    assert_eq!(s.as_slice(), b"test data");
    assert!(v.is_heap());
}

#[test]
fn page_growth_when_full() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);

    // Fill a 4K page: (4096 - 8) / 48 = ~85 objects max
    // Allocate enough to spill into a second page.
    for _ in 0..100 {
        rp.alloc_obj(cons_obj(), &mut pool);
    }
    assert!(rp.pages.len() >= 2, "should have claimed multiple pages");
    assert_eq!(rp.obj_count(), 100);
}

#[test]
fn geometric_page_size_is_capped() {
    // A region that keeps needing new pages grows its page size
    // geometrically. Uncapped, that doubling races to hundreds of MB
    // / multi-GB pages — and a single small allocation that spills
    // onto a fresh page then claims a giant page for a few bytes.
    // (This is what made `(apply concat …)` of small chunks balloon a
    // region to multiple GB and trip the region_of_page_ptr loop cap.)
    // The growth must saturate at MAX_PAGE_SIZE.
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);

    // Allocate past the cap so the doubling would, uncapped, claim a
    // page strictly larger than MAX_PAGE_SIZE (the next size after the
    // first full MAX_PAGE_SIZE page).
    let chunk = [0u8; 1024];
    let mut total = 0usize;
    while total <= MAX_PAGE_SIZE + MAX_PAGE_SIZE / 4 {
        rp.alloc_region_slice(&chunk, &mut pool);
        total += chunk.len();
    }

    let max_page = rp.pages.iter().map(|p| p.page.len()).max().unwrap();
    assert!(
        max_page <= MAX_PAGE_SIZE,
        "largest page {} exceeds MAX_PAGE_SIZE {} — geometric growth not capped",
        max_page,
        MAX_PAGE_SIZE,
    );
    assert!(
        rp.next_page_size <= MAX_PAGE_SIZE,
        "next_page_size {} exceeds MAX_PAGE_SIZE {}",
        rp.next_page_size,
        MAX_PAGE_SIZE,
    );
}

#[test]
fn teardown_returns_pages() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    for _ in 0..10 {
        rp.alloc_obj(cons_obj(), &mut pool);
    }
    rp.teardown(&mut pool);
    assert_eq!(rp.obj_count(), 0);
    assert!(rp.pages.is_empty());
    // Pages should be in the pool's cache now.
    assert!(pool.cached_bytes() > 0);
}

#[test]
fn teardown_runs_dtors() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut pool = PagePool::default();
    let mut rp = pool_for(1);

    // Allocate an LBox (needs Drop for Rc).
    let cell = Rc::new(RefCell::new(Value::NIL));
    let weak = Rc::downgrade(&cell);
    let obj = HeapObject::LBox {
        cell,
        traits: Value::NIL,
    };
    let _v = rp.alloc_obj(obj, &mut pool);
    assert_eq!(rp.dtors.len(), 1);

    rp.teardown(&mut pool);
    // After teardown, the Rc should have been dropped.
    assert!(weak.upgrade().is_none(), "Rc should be dropped by teardown");
}

#[test]
fn alloc_empty_region_slice() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    let s = rp.alloc_region_slice::<u8>(&[], &mut pool);
    assert_eq!(s.as_slice(), &[] as &[u8]);
}

#[test]
fn owns_detects_region_pointers() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    let ptr = v.as_heap_ptr().unwrap();
    assert!(rp.owns(ptr));

    let x: i64 = 42;
    assert!(!rp.owns(&x as *const _ as *const ()));
}

#[test]
fn page_header_region_id() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(42);
    let v = rp.alloc_obj(cons_obj(), &mut pool);

    // Read region_id from the page header.
    let ptr = v.as_heap_ptr().unwrap();
    let rid = unsafe { region_of_page_ptr(ptr, 4096) };
    assert_eq!(rid, 42);
}

#[test]
fn page_header_stamp_roundtrip() {
    // Every claimed page is stamped (region_id, generation, store) at
    // claim time (docs/impl/region/generations.md § "Region generations").
    let mut pool = PagePool::default();
    let stamp = PageStamp {
        generation: 7,
        store: 9,
    };
    let mut rp = RegionPool::new(42, stamp, 4096);
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    let ptr = v.as_heap_ptr().unwrap();
    let (rid, read_back) = unsafe { header_of_page_ptr(ptr, 4096) };
    assert_eq!(rid, 42);
    assert_eq!(read_back, stamp);
}

#[test]
fn large_inline_data_claims_bigger_page() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    // Allocate data larger than a 4K page.
    let big_data = vec![0xABu8; 8000];
    let s = rp.alloc_region_slice(&big_data, &mut pool);
    assert_eq!(s.as_slice(), &big_data[..]);
}

#[test]
fn region_of_page_ptr_on_grown_page() {
    // After geometric growth, objects may be on pages larger than 4K.
    // region_of_page_ptr must still find the correct region ID.
    let mut pool = PagePool::default();
    let mut rp = pool_for(99);

    // Fill the first 4K page to force a second (8K) page.
    for _ in 0..100 {
        rp.alloc_obj(cons_obj(), &mut pool);
    }
    assert!(rp.pages.len() >= 2, "should have grown to multiple pages");

    // Allocate on the grown page.
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    let ptr = v.as_heap_ptr().unwrap();
    let rid = unsafe { region_of_page_ptr(ptr, 4096) };
    assert_eq!(rid, 99, "region_of_page_ptr must work on grown pages");
}

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
    // Counterfactual: pre-magic, `header_of_page_ptr` matched the forged 8 KiB
    // sub-base (its byte 12 equals log2 8192) and returned the forged region id;
    // post-magic the forged tag lacks the magic, so the walk reaches the true
    // 16 KiB base and returns region 42.
    let mut pool = PagePool::new(BASE_PAGE, 8 * 1024 * 1024);
    // A 16 KiB page (log2 = 14), self-aligned by `claim`.
    let page = pool.claim(4 * BASE_PAGE);
    assert_eq!(page.len(), 4 * BASE_PAGE);
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
    // Forge a "smaller page" header mid-page, at the 8 KiB-aligned sub-base: a
    // garbage region id, and a bare `log2(8192)` at the `size_tag` offset (12)
    // WITHOUT the magic — exactly the mid-page object data that fooled the old
    // single-byte check.
    let sub_base = base + 2 * BASE_PAGE; // 8 KiB in — 8 KiB-aligned
    unsafe {
        *(sub_base as *mut u32) = 0xDEAD_BEEF; // forged region_id
        *((sub_base + 12) as *mut u32) = (2 * BASE_PAGE).trailing_zeros(); // bare log2, no magic
    }
    // A pointer deep in the page, past the forged 8 KiB sub-base.
    let deep = (sub_base + 0x620) as *const ();
    let rid = unsafe { region_of_page_ptr(deep, BASE_PAGE) };
    assert_eq!(
        rid, 42,
        "region_of_page_ptr resolved a deep pointer to a mid-page false header \
         (got 0x{rid:x}) instead of the page's true base region 42 — the size_tag \
         magic must reject a sub-aligned mid-page match"
    );
}
