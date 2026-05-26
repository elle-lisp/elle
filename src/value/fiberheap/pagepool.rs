//! Per-thread page cache for region allocation.
//!
//! `PagePool` caches mmap'd pages organized by size class. When a region
//! needs a new page, it claims one from the pool (or mmaps fresh). When a
//! region is freed, its pages are returned to the pool for reuse. Pages
//! exceeding the cache limit are munmapped immediately.

/// An mmap-backed page of memory with a known size.
///
/// On Drop, the page is munmapped — the OS reclaims the physical memory
/// immediately with no allocator caching layer.
pub(crate) struct MmapPage {
    ptr: *mut u8,
    len: usize,
}

impl MmapPage {
    /// Allocate `len` bytes of zero-initialized, self-aligned memory.
    ///
    /// Self-aligned means the returned address is a multiple of `len`.
    /// This is required by `region_of_page_ptr`, which masks a pointer
    /// with `!(len - 1)` to find the page base.
    ///
    /// For 4K pages, `mmap` already guarantees 4K alignment. For larger
    /// pages we over-allocate 2× and trim (munmap prefix/suffix) to get
    /// a `len`-aligned sub-range.
    fn new(len: usize) -> Option<Self> {
        debug_assert!(len >= 4096 && len.is_power_of_two());
        if len == 4096 {
            return Self::new_raw(len);
        }
        // Over-allocate to guarantee len-alignment.
        let alloc = len * 2;
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                alloc,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return None;
        }
        let addr = raw as usize;
        let aligned = (addr + len - 1) & !(len - 1);
        let prefix = aligned - addr;
        let suffix = alloc - prefix - len;
        unsafe {
            if prefix > 0 {
                libc::munmap(raw, prefix);
            }
            if suffix > 0 {
                libc::munmap((aligned + len) as *mut libc::c_void, suffix);
            }
        }
        Some(MmapPage {
            ptr: aligned as *mut u8,
            len,
        })
    }

    /// Raw mmap without alignment trimming (used for 4K pages).
    fn new_raw(len: usize) -> Option<Self> {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            None
        } else {
            Some(MmapPage {
                ptr: ptr as *mut u8,
                len,
            })
        }
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Advise the kernel that page contents are no longer needed.
    pub fn discard_contents(&self) {
        unsafe {
            libc::madvise(self.ptr as *mut libc::c_void, self.len, libc::MADV_DONTNEED);
        }
    }
}

impl Drop for MmapPage {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.len);
        }
    }
}

// SAFETY: MmapPage owns its virtual memory exclusively.
unsafe impl Send for MmapPage {}

/// Size class index for a given page size.
/// Classes are powers of two: 4K=0, 8K=1, 16K=2, 32K=3, 64K=4, etc.
fn size_class(size: usize) -> usize {
    debug_assert!(size >= 4096 && size.is_power_of_two());
    size.trailing_zeros() as usize - 12 // 4096 = 2^12 → class 0
}

#[allow(dead_code)]
fn class_size(class: usize) -> usize {
    4096 << class
}

/// Number of size classes to track (4K through 4MB = 11 classes).
const NUM_CLASSES: usize = 11;

/// Per-thread page cache.
///
/// Caches pages by size class for reuse across region lifetimes.
/// When the cache exceeds `max_cached` bytes, excess pages are munmapped.
pub(crate) struct PagePool {
    /// Free lists per size class (index by `size_class()`).
    free_lists: Vec<Vec<MmapPage>>,
    /// Total cached bytes across all size classes.
    cached_bytes: usize,
    /// Maximum bytes to cache. Pages beyond this are munmapped on release.
    max_cached: usize,
    /// Initial page size for new regions (CLI: --region-page-size).
    initial_page_size: usize,
}

impl PagePool {
    pub fn new(initial_page_size: usize, max_cached: usize) -> Self {
        PagePool {
            free_lists: (0..NUM_CLASSES).map(|_| Vec::new()).collect(),
            cached_bytes: 0,
            max_cached,
            initial_page_size,
        }
    }

    /// Initial page size for new regions.
    #[inline]
    pub fn initial_page_size(&self) -> usize {
        self.initial_page_size
    }

    /// Claim a page of exactly `size` bytes.
    ///
    /// Pops from the free list if available (O(1)), otherwise mmaps fresh.
    /// The page's contents are discarded (zero-filled by madvise or fresh mmap).
    pub fn claim(&mut self, size: usize) -> MmapPage {
        let size = size.next_power_of_two().max(4096);
        let class = size_class(size);
        if class < NUM_CLASSES {
            if let Some(page) = self.free_lists[class].pop() {
                self.cached_bytes -= page.len();
                page.discard_contents();
                return page;
            }
        }
        MmapPage::new(size).expect("pagepool: mmap failed")
    }

    /// Return a single page to the cache.
    ///
    /// If adding this page would exceed the cache limit, it is munmapped
    /// instead (dropped immediately).
    pub fn release(&mut self, page: MmapPage) {
        let size = page.len();
        let class = size_class(size.next_power_of_two().max(4096));
        if class < NUM_CLASSES && self.cached_bytes + size <= self.max_cached {
            self.cached_bytes += size;
            self.free_lists[class].push(page);
        }
        // else: page is dropped here → munmap
    }

    #[allow(dead_code)]
    pub fn release_batch(&mut self, pages: Vec<MmapPage>) {
        for page in pages {
            self.release(page);
        }
    }

    /// Total cached bytes.
    pub fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }
}

impl Default for PagePool {
    fn default() -> Self {
        Self::new(4096, 4 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_returns_valid_page() {
        let mut pool = PagePool::default();
        let page = pool.claim(4096);
        assert_eq!(page.len(), 4096);
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
        let page = pool.claim(4096);
        let ptr = page.as_ptr();
        pool.release(page);
        assert_eq!(pool.cached_bytes(), 4096);

        let page2 = pool.claim(4096);
        // Should reuse the cached page (same address)
        assert_eq!(page2.as_ptr(), ptr);
        assert_eq!(pool.cached_bytes(), 0);
    }

    #[test]
    fn release_batch_caches_all() {
        let mut pool = PagePool::default();
        let pages: Vec<MmapPage> = (0..3).map(|_| pool.claim(4096)).collect();
        pool.release_batch(pages);
        assert_eq!(pool.cached_bytes(), 3 * 4096);
    }

    #[test]
    fn cache_limit_drops_excess() {
        // max_cached = 8192 → can hold two 4K pages
        let mut pool = PagePool::new(4096, 8192);
        let p1 = pool.claim(4096);
        let p2 = pool.claim(4096);
        let p3 = pool.claim(4096);
        pool.release(p1);
        pool.release(p2);
        assert_eq!(pool.cached_bytes(), 8192);
        // Third release exceeds limit → dropped (munmapped)
        pool.release(p3);
        assert_eq!(pool.cached_bytes(), 8192);
    }

    #[test]
    fn geometric_growth_sizes() {
        let mut pool = PagePool::new(4096, 4 * 1024 * 1024);
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
        let mut page = pool.claim(4096);
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
}
