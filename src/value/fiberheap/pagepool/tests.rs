//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn claim_returns_valid_page() {
    let mut pool = PagePool::default();
    let page = pool.claim(BASE_PAGE);
    assert_eq!(page.len(), BASE_PAGE);
    assert!(!page.as_ptr().is_null());
}

#[test]
fn claim_rounds_up_to_power_of_two() {
    let mut pool = PagePool::default();
    let page = pool.claim(5000); // rounds to 8192
    assert_eq!(page.len(), 8192);
}

/// A page whose whole span the caller may have written — what a test that does
/// not care about spans hands to `release`.
fn all_of(page: &MmapPage) -> PageDirty {
    PageDirty::whole(page.len())
}

#[test]
fn release_and_reclaim() {
    let mut pool = PagePool::default();
    let page = pool.claim(BASE_PAGE);
    let ptr = page.as_ptr();
    let dirty = all_of(&page);
    pool.release(page, dirty);
    assert_eq!(pool.cached_bytes(), BASE_PAGE);

    let page2 = pool.claim(BASE_PAGE);
    // Should reuse the cached page (same address)
    assert_eq!(page2.as_ptr(), ptr);
    assert_eq!(pool.cached_bytes(), 0);
}

#[test]
fn cache_limit_drops_excess() {
    // max_cached = 8192 → can hold two 4K pages
    let mut pool = PagePool::new(BASE_PAGE, 8192);
    let p1 = pool.claim(BASE_PAGE);
    let p2 = pool.claim(BASE_PAGE);
    let p3 = pool.claim(BASE_PAGE);
    let (d1, d2, d3) = (all_of(&p1), all_of(&p2), all_of(&p3));
    pool.release(p1, d1);
    pool.release(p2, d2);
    assert_eq!(pool.cached_bytes(), 8192);
    // Third release exceeds limit → dropped (munmapped)
    pool.release(p3, d3);
    assert_eq!(pool.cached_bytes(), 8192);
}

#[test]
fn geometric_growth_sizes() {
    let mut pool = PagePool::new(BASE_PAGE, 4 * 1024 * 1024);
    // Simulate geometric doubling
    let mut size = pool.initial_page_size();
    for _ in 0..5 {
        let page = pool.claim(size);
        assert_eq!(page.len(), size);
        let dirty = all_of(&page);
        pool.release(page, dirty);
        size *= 2;
    }
    // sizes: 4K, 8K, 16K, 32K, 64K
    assert_eq!(size, 128 * 1024);
}

// ── The page-recycle contract (docs/impl/region/model.md § "Page recycling") ──
//
// Recycling a page is a free-list pop. The pool neither reads nor writes the
// page on the way through, in either direction, so a region pays for a page
// once — when it writes it. The Elle-level companion is
// tests/elle/region-page-recycle.lisp, which measures what a claim costs a
// running program through the `arena/page-claims` gauge.
//
// The `--trace=scrub` diagnostic is the one thing that does write a released
// page, and it writes exactly the spans `PageDirty` names. Those spans are
// tested here against `MmapPage::reset` directly; that the diagnostic leaves a
// correct program correct is tests/region_scrub.rs, which arms the trace flag
// process-wide and so needs its own test binary.

/// The page header `RegionPage` writes at offset 0 — the span a scrub keeps.
const HEADER: usize = 16;

#[test]
fn claiming_a_recycled_page_touches_no_page_byte() {
    // Mark the cached page behind the pool's back and claim it. Anything the
    // claim path might do to the page — a `memset`, a
    // `madvise(MADV_DONTNEED)` — erases the mark; a free-list pop hands it
    // back exactly as released. This is the hot path of a short-lived region
    // holding one small object, and it must cost nothing per page.
    let mut pool = PagePool::default();
    let page = pool.claim(BASE_PAGE);
    pool.release(page, PageDirty::new(HEADER..64, BASE_PAGE..BASE_PAGE));
    let cached = pool
        .peek_cached(BASE_PAGE)
        .expect("the released page must be in the cache");
    unsafe { std::ptr::write(cached.as_mut_ptr().add(HEADER), 0x5A) };

    let recycled = pool.claim(BASE_PAGE);
    assert_eq!(
        pool.counters().recycles(),
        1,
        "the second claim must recycle"
    );
    assert_eq!(
        unsafe { std::ptr::read(recycled.as_ptr().add(HEADER)) },
        0x5A,
        "claim wrote over the page it recycled; a claimant stamps the header \
         and writes every slot it hands out, so the page needs no preparation \
         and must not cost a kernel round trip",
    );
}

#[test]
fn reset_covers_the_written_spans_and_spares_the_header() {
    // A region's page has two written spans — the object slots and the
    // inline-data suffix — with an untouched gap between them. A reset clears
    // both and nothing else, so a page holding one 48-byte object behind a
    // 16-byte header costs 48 bytes of work rather than 4096.
    //
    // The header is spared on purpose: a cached page keeps the
    // `(region, generation, store)` stamp of the region that died on it, which
    // is what a pointer outliving that region resolves through and what makes
    // the generation mismatch a panic at the deref site
    // (docs/impl/region/generations.md).
    let mut pool = PagePool::default();
    let mut page = pool.claim(BASE_PAGE);
    unsafe {
        std::ptr::write_bytes(page.as_mut_ptr(), 0xFF, HEADER);
        std::ptr::write_bytes(page.as_mut_ptr().add(HEADER), 0xAB, 48);
        std::ptr::write_bytes(page.as_mut_ptr().add(BASE_PAGE - 100), 0xCD, 100);
    }

    let written = page.reset(&PageDirty::new(HEADER..64, BASE_PAGE - 100..BASE_PAGE));
    assert_eq!(written, 48 + 100);

    let bytes = unsafe { std::slice::from_raw_parts(page.as_ptr(), BASE_PAGE) };
    assert_eq!(
        &bytes[..HEADER],
        [0xFFu8; HEADER],
        "the reset blanked the page header, so a stale pointer into this page \
         no longer resolves to the region that died on it",
    );
    let left = bytes[HEADER..].iter().position(|&b| b != 0);
    assert_eq!(
        left, None,
        "the reset left body byte {left:?} set — a scrubbed page must read as \
         zero everywhere its region wrote",
    );
}

#[test]
fn reset_of_an_untouched_page_writes_nothing() {
    // A region that claimed a page and died before allocating into it wrote
    // only the header, which the reset spares. The spans are then empty.
    let mut pool = PagePool::default();
    let mut page = pool.claim(BASE_PAGE);
    let written = page.reset(&PageDirty::new(HEADER..HEADER, BASE_PAGE..BASE_PAGE));
    assert_eq!(written, 0);
}

#[test]
fn write_and_read_page_contents() {
    let mut pool = PagePool::default();
    let mut page = pool.claim(BASE_PAGE);
    // Write to the page
    unsafe {
        let ptr = page.as_mut_ptr();
        std::ptr::write(ptr as *mut u64, 0xDEAD_BEEF_CAFE_BABE);
        assert_eq!(std::ptr::read(ptr as *const u64), 0xDEAD_BEEF_CAFE_BABE);
    }
}

#[test]
fn large_pages_are_self_aligned() {
    for &size in &[8192, 16384, 32768, 65536] {
        let page = MmapPage::new(size).unwrap();
        assert_eq!(
            page.as_ptr() as usize % size,
            0,
            "{size}-byte page not self-aligned"
        );
    }
}
