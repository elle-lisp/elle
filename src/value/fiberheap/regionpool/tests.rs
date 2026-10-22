// audited: 2026-10-21
//! Unit tests for one region's pages: dual-ended layout, growth, teardown,
//! the walk that finds a page base, and the cross-region scan.
//!
//! docs/impl/region/model.md
//! docs/impl/region/generations.md

use super::super::pagepool::base_page;
use super::*;
use crate::value::heap::Pair;

/// A pool for `region_id` with a default (generation 0, store 0) stamp and an
/// isolated (all-zero) trace cell — these unit tests never exercise the `PAGES`
/// gate.
fn pool_for(region_id: u32) -> RegionPool {
    RegionPool::new(
        region_id,
        PageStamp::default(),
        base_page(),
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    )
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

    // Both should be on the same page.
    assert_eq!(rp.pages.len(), 1);
    assert_eq!(s.as_slice(), b"test data");
    assert!(v.is_heap());
}

#[test]
fn page_growth_when_full() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);

    // One object slot per `HeapObject`, so a page holds at most
    // `base_page() / size_of::<HeapObject>()` of them. Ask for one more than a
    // full page's worth, on whatever page size the host uses.
    let spill = base_page() / size_of::<HeapObject>() + 1;
    for _ in 0..spill {
        rp.alloc_obj(cons_obj(), &mut pool);
    }
    assert!(rp.pages.len() >= 2, "should have claimed multiple pages");
    assert_eq!(rp.obj_count(), spill);
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
fn teardown_returns_a_page_that_still_names_its_region() {
    // A region hands each page back through the cursors it filled
    // (docs/impl/region/model.md). The pool caches the page
    // as it stands, header included, so a pointer that outlived this region
    // still resolves to region 42 and trips the generation check rather than
    // reading a page that names nothing (docs/impl/region/generations.md).
    let mut pool = PagePool::default();
    let mut rp = pool_for(42);
    rp.alloc_obj(cons_obj(), &mut pool);
    rp.alloc_region_slice(b"inline payload", &mut pool);
    rp.teardown(&mut pool);

    let cached = pool
        .peek_cached(base_page())
        .expect("teardown must return the page to the cache");
    let rid = unsafe { region_of_page_ptr(cached.as_ptr() as *const (), base_page()) };
    assert_eq!(
        rid, 42,
        "the cached page lost the stamp of the region that died on it, so a \
         stale pointer into it no longer names region 42",
    );
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
    let rid = unsafe { region_of_page_ptr(ptr, base_page()) };
    assert_eq!(rid, 42);
}

#[test]
fn page_header_stamp_roundtrip() {
    // Every claimed page is stamped (region_id, generation, store) at
    // claim time (docs/impl/region/generations.md).
    let mut pool = PagePool::default();
    let stamp = PageStamp {
        generation: 7,
        store: 9,
    };
    let mut rp = RegionPool::new(
        42,
        stamp,
        base_page(),
        std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
    );
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    let ptr = v.as_heap_ptr().unwrap();
    let (rid, read_back) = unsafe { header_of_page_ptr(ptr, base_page()) };
    assert_eq!(rid, 42);
    assert_eq!(read_back, stamp);
}

#[test]
fn large_inline_data_claims_bigger_page() {
    let mut pool = PagePool::default();
    let mut rp = pool_for(1);
    // Allocate data larger than the region's first page.
    let big_data = vec![0xABu8; 2 * base_page()];
    let s = rp.alloc_region_slice(&big_data, &mut pool);
    assert_eq!(s.as_slice(), &big_data[..]);
}

#[test]
fn region_of_page_ptr_on_grown_page() {
    // After geometric growth, objects may be on pages larger than the base
    // page. region_of_page_ptr must still find the correct region ID.
    let mut pool = PagePool::default();
    let mut rp = pool_for(99);

    // Fill the first page to force a second, double-sized one.
    let mut allocated = 0;
    while rp.pages.len() < 2 {
        rp.alloc_obj(cons_obj(), &mut pool);
        allocated += 1;
        assert!(
            allocated < 100_000,
            "the region never claimed a second page"
        );
    }

    // Allocate on the grown page.
    let v = rp.alloc_obj(cons_obj(), &mut pool);
    let ptr = v.as_heap_ptr().unwrap();
    let rid = unsafe { region_of_page_ptr(ptr, base_page()) };
    assert_eq!(rid, 99, "region_of_page_ptr must work on grown pages");
}

/// A string or bytes whose payload lies in another region's pages holds that
/// region: the scan reports the backing's region, as it does a shared closure
/// env (docs/impl/values.md).
///
/// The trap: the scan's string and bytes arms once enumerated nothing, which
/// is right for an inline payload and frees a view's source under it.
#[test]
fn a_slice_backing_in_another_region_is_a_cross_region_reference() {
    let mut pool = PagePool::default();
    let mut source = pool_for(7);
    let mut holder = pool_for(9);
    let backing = source.alloc_region_slice(b"a line of bytes", &mut pool);
    let inline = holder.alloc_region_slice(b"inline", &mut pool);
    // The owner resolver a store would pass: the region that owns the
    // address, or 0 when neither does.
    let owner = |ptr: *const ()| {
        if source.owns(ptr) {
            7
        } else if holder.owns(ptr) {
            9
        } else {
            0
        }
    };
    let refs_of = |obj: &HeapObject| {
        let mut refs = Vec::new();
        RegionPool::find_object_cross_refs(obj, 9, &owner, &mut refs);
        refs
    };

    // A view two bytes into the source's payload, held in region 9.
    let view = unsafe { RegionSlice::from_raw(backing.as_ptr().add(2), 4) };
    assert_eq!(
        refs_of(&HeapObject::LBytes {
            data: view,
            traits: Value::NIL
        }),
        vec![7],
        "a bytes view must hold its source's region"
    );
    assert_eq!(
        refs_of(&HeapObject::LString {
            s: view,
            traits: Value::NIL
        }),
        vec![7],
        "a string view must hold its source's region"
    );
    assert!(
        refs_of(&HeapObject::LBytes {
            data: inline,
            traits: Value::NIL
        })
        .is_empty(),
        "an inline payload is a self-edge and holds nothing"
    );
    assert!(
        refs_of(&HeapObject::LBytes {
            data: RegionSlice::empty(),
            traits: Value::NIL
        })
        .is_empty(),
        "an empty payload has no page to resolve"
    );
}
