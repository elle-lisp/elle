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

#[test]
fn release_and_reclaim() {
    let mut pool = PagePool::default();
    let page = pool.claim(BASE_PAGE);
    let ptr = page.as_ptr();
    pool.release(page);
    assert_eq!(pool.cached_bytes(), BASE_PAGE);

    let page2 = pool.claim(BASE_PAGE);
    // Should reuse the cached page (same address)
    assert_eq!(page2.as_ptr(), ptr);
    assert_eq!(pool.cached_bytes(), 0);
}

#[test]
fn release_batch_caches_all() {
    let mut pool = PagePool::default();
    let pages: Vec<MmapPage> = (0..3).map(|_| pool.claim(BASE_PAGE)).collect();
    pool.release_batch(pages);
    assert_eq!(pool.cached_bytes(), 3 * BASE_PAGE);
}

#[test]
fn cache_limit_drops_excess() {
    // max_cached = 8192 → can hold two 4K pages
    let mut pool = PagePool::new(BASE_PAGE, 8192);
    let p1 = pool.claim(BASE_PAGE);
    let p2 = pool.claim(BASE_PAGE);
    let p3 = pool.claim(BASE_PAGE);
    pool.release(p1);
    pool.release(p2);
    assert_eq!(pool.cached_bytes(), 8192);
    // Third release exceeds limit → dropped (munmapped)
    pool.release(p3);
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
        pool.release(page);
        size *= 2;
    }
    // sizes: 4K, 8K, 16K, 32K, 64K
    assert_eq!(size, 128 * 1024);
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
