//! Unit tests (`super` is the parent impl module).

use super::*;

/// The host's page size, asked of the OS here rather than taken from the module
/// under test — the independent expectation the ladder must agree with.
fn os_page() -> usize {
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(n > 0, "sysconf(_SC_PAGESIZE) answered {n}");
    n as usize
}

#[test]
fn claim_returns_valid_page() {
    let mut pool = PagePool::default();
    let page = pool.claim(base_page());
    assert_eq!(page.len(), base_page());
    assert!(!page.as_ptr().is_null());
}

#[test]
fn claim_rounds_up_to_power_of_two() {
    let mut pool = PagePool::default();
    // A request between two classes takes the larger one.
    let page = pool.claim(base_page() + 1);
    assert_eq!(page.len(), 2 * base_page());
}

// ── The base page is the OS page (docs/impl/region/model.md § "The base page
// is the OS page") ──

#[test]
fn base_page_is_the_os_page() {
    assert_eq!(
        base_page(),
        os_page().max(MIN_BASE_PAGE),
        "the smallest region page must be the page the kernel actually charges \
         for; a smaller one costs a whole OS page and accounts for a fraction \
         of it",
    );
}

#[test]
fn derive_base_page_takes_the_os_answer_and_floors_it() {
    // The three page sizes a supported host reports: 4096 (Linux x86-64, and
    // Linux aarch64 as usually built), 16384 (macOS aarch64, Linux aarch64
    // built for 16K), 65536 (Linux aarch64 built for 64K).
    assert_eq!(derive_base_page(4096), 4096);
    assert_eq!(derive_base_page(16384), 16384);
    assert_eq!(derive_base_page(65536), 65536);
    // The floor covers an answer no region page could be built from: a page
    // below the floor, and the -1 `sysconf` returns when it cannot answer.
    assert_eq!(derive_base_page(512), MIN_BASE_PAGE);
    assert_eq!(derive_base_page(-1), MIN_BASE_PAGE);
    assert_eq!(derive_base_page(0), MIN_BASE_PAGE);
    // A non-power-of-two is not a page size any of the masking arithmetic can
    // use, so it takes the floor rather than corrupting the ladder.
    assert_eq!(derive_base_page(6000), MIN_BASE_PAGE);
}

/// The hypothetical hosts the ladder is checked against. The host running the
/// test only ever exercises its own page size, and 4096 is the one that hides
/// every consequence of a fixed base.
const HOST_PAGES: [libc::c_long; 3] = [4096, 16384, 65536];

#[test]
fn every_size_class_is_a_whole_number_of_os_pages() {
    // The counter-factual: a ladder rooted at a fixed 4096 puts 4096 and 8192
    // on it. On a 16384-byte-page host the kernel charges a full 16384 for
    // each, so classes 0, 1 and 2 are one physical size filed under three free
    // lists, and `cached_bytes` counts a quarter of what class 0 holds.
    for os in HOST_PAGES {
        let base = derive_base_page(os);
        let os = os as usize;
        for class in 0..NUM_CLASSES {
            let len = class_size_of(base, class);
            assert_eq!(
                len % os,
                0,
                "class {class} of a base-{base} ladder is {len} bytes, not a \
                 whole number of {os}-byte OS pages",
            );
            assert_eq!(
                size_class_of(base, len),
                class,
                "class {class} does not round-trip through its size {len}",
            );
        }
    }
}

#[test]
fn a_trim_gives_back_whole_os_pages() {
    // The trap: `munmap` refuses an address that is not page-aligned. Trimming
    // an over-allocation down to an 8192-byte page on a 16384-byte-page host
    // hands it `base + 8192` and gets EINVAL, and the return value is the only
    // place that shows. Whole-OS-page classes are what makes every trim legal.
    for os in HOST_PAGES {
        let base = derive_base_page(os);
        let os = os as usize;
        for class in 0..NUM_CLASSES {
            let len = class_size_of(base, class);
            let alloc = 2 * len;
            // `mmap` answers with an OS-page-aligned address; sweep the offsets
            // within one class that the kernel could pick.
            for step in 0..(len / os).max(1) {
                let raw = 16 * len + step * os;
                let trim = Trim::new(raw, alloc, len);
                assert_eq!(trim.base % len, 0, "trimmed base is not self-aligned");
                assert_eq!(
                    trim.prefix + len + trim.suffix,
                    alloc,
                    "the trim loses or invents bytes",
                );
                assert_eq!(
                    trim.prefix % os,
                    0,
                    "munmap would refuse the {}-byte prefix at {raw:#x} on an \
                     {os}-byte-page host",
                    trim.prefix,
                );
                assert_eq!(
                    (trim.base + len) % os,
                    0,
                    "munmap would refuse the suffix address for a {len}-byte \
                     page on an {os}-byte-page host",
                );
                assert_eq!(trim.suffix % os, 0, "the suffix is a partial OS page");
            }
        }
    }
}

#[test]
fn the_smallest_claim_is_one_whole_os_page() {
    // A one-byte region still costs a page from the kernel. The pool must hand
    // out — and account for — exactly what the kernel charges.
    let mut pool = PagePool::default();
    let page = pool.claim(1);
    assert_eq!(
        page.len(),
        os_page().max(MIN_BASE_PAGE),
        "the smallest page the pool hands out is not the page the kernel maps",
    );
}

#[test]
fn a_released_page_is_cached_at_the_size_the_kernel_charged() {
    // `cached_bytes` is what `--page-pool-max` bounds. It counts the page's
    // recorded length, so that length has to be the mapping's real size.
    let mut pool = PagePool::default();
    let page = pool.claim(1);
    let len = page.len();
    let dirty = all_of(&page);
    pool.release(page, dirty);
    assert_eq!(pool.cached_bytes(), len);
    assert_eq!(
        len % os_page(),
        0,
        "a cached page of {len} bytes holds a fraction of an OS page, so the \
         pool retains more memory than `--page-pool-max` names",
    );
}

/// A page whose whole span the caller may have written — what a test that does
/// not care about spans hands to `release`.
fn all_of(page: &MmapPage) -> PageDirty {
    PageDirty::whole(page.len())
}

#[test]
fn release_and_reclaim() {
    let mut pool = PagePool::default();
    let page = pool.claim(base_page());
    let ptr = page.as_ptr();
    let dirty = all_of(&page);
    pool.release(page, dirty);
    assert_eq!(pool.cached_bytes(), base_page());

    let page2 = pool.claim(base_page());
    // Should reuse the cached page (same address)
    assert_eq!(page2.as_ptr(), ptr);
    assert_eq!(pool.cached_bytes(), 0);
}

#[test]
fn cache_limit_drops_excess() {
    // A cache with room for exactly two base pages.
    let cap = 2 * base_page();
    let mut pool = PagePool::new(base_page(), cap);
    let p1 = pool.claim(base_page());
    let p2 = pool.claim(base_page());
    let p3 = pool.claim(base_page());
    let (d1, d2, d3) = (all_of(&p1), all_of(&p2), all_of(&p3));
    pool.release(p1, d1);
    pool.release(p2, d2);
    assert_eq!(pool.cached_bytes(), cap);
    // Third release exceeds limit → dropped (munmapped)
    pool.release(p3, d3);
    assert_eq!(pool.cached_bytes(), cap);
}

#[test]
fn geometric_growth_sizes() {
    let mut pool = PagePool::new(base_page(), 4 * 1024 * 1024);
    // Simulate geometric doubling
    let mut size = pool.initial_page_size();
    for _ in 0..5 {
        let page = pool.claim(size);
        assert_eq!(page.len(), size);
        let dirty = all_of(&page);
        pool.release(page, dirty);
        size *= 2;
    }
    // Five doublings from the base page.
    assert_eq!(size, base_page() << 5);
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
    let page = pool.claim(base_page());
    pool.release(page, PageDirty::new(HEADER..64, base_page()..base_page()));
    let cached = pool
        .peek_cached(base_page())
        .expect("the released page must be in the cache");
    unsafe { std::ptr::write(cached.as_mut_ptr().add(HEADER), 0x5A) };

    let recycled = pool.claim(base_page());
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
    // 16-byte header costs 48 bytes of work rather than a whole page.
    //
    // The header is spared on purpose: a cached page keeps the
    // `(region, generation, store)` stamp of the region that died on it, which
    // is what a pointer outliving that region resolves through and what makes
    // the generation mismatch a panic at the deref site
    // (docs/impl/region/generations.md).
    let mut pool = PagePool::default();
    let mut page = pool.claim(base_page());
    let len = page.len();
    unsafe {
        std::ptr::write_bytes(page.as_mut_ptr(), 0xFF, HEADER);
        std::ptr::write_bytes(page.as_mut_ptr().add(HEADER), 0xAB, 48);
        std::ptr::write_bytes(page.as_mut_ptr().add(len - 100), 0xCD, 100);
    }

    let written = page.reset(&PageDirty::new(HEADER..64, len - 100..len));
    assert_eq!(written, 48 + 100);

    let bytes = unsafe { std::slice::from_raw_parts(page.as_ptr(), len) };
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
    let mut page = pool.claim(base_page());
    let written = page.reset(&PageDirty::new(HEADER..HEADER, base_page()..base_page()));
    assert_eq!(written, 0);
}

#[test]
fn write_and_read_page_contents() {
    let mut pool = PagePool::default();
    let mut page = pool.claim(base_page());
    // Write to the page
    unsafe {
        let ptr = page.as_mut_ptr();
        std::ptr::write(ptr as *mut u64, 0xDEAD_BEEF_CAFE_BABE);
        assert_eq!(std::ptr::read(ptr as *const u64), 0xDEAD_BEEF_CAFE_BABE);
    }
}

#[test]
fn large_pages_are_self_aligned() {
    // `region_of_page_ptr` finds a page's base by masking a pointer with
    // `!(len - 1)`, so every page above class 0 has to be aligned to its own
    // length — which `mmap` alone does not give.
    for class in 1..5 {
        let size = class_size_of(base_page(), class);
        let page = MmapPage::new(size).unwrap();
        assert_eq!(
            page.as_ptr() as usize % size,
            0,
            "{size}-byte page not self-aligned"
        );
    }
}

// A file-backed (hydrated image) page is never cached: its release is
// `munmap`, even when the cache has room (docs/impl/image.md § Hydration
// step 3). The anonymous release above (`release_and_reclaim`) is the
// counter-factual: same pool, same room, an anonymous page IS cached.
#[test]
fn file_backed_release_is_munmap_not_cache() {
    let mut pool = PagePool::default();
    // A self-aligned anonymous mapping standing in for a hydrated image
    // page (a private file view behaves identically at release).
    let raw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            base_page(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(raw, libc::MAP_FAILED);
    let page = unsafe { MmapPage::from_fixed_mapping(raw as *mut u8, base_page()) };
    let dirty = all_of(&page);
    pool.release(page, dirty);
    assert_eq!(
        pool.cached_bytes(),
        0,
        "file-backed page entered the cache instead of being unmapped"
    );
}
