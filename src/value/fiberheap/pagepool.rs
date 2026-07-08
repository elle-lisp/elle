//! Per-thread page cache for region allocation.
//!
//! `PagePool` caches mmap'd pages organized by size class. When a region
//! needs a new page, it claims one from the pool (or mmaps fresh). When a
//! region is freed, its pages are returned to the pool for reuse. Pages
//! exceeding the cache limit are munmapped immediately.

/// The OS base page size — the smallest region page and the unit that `mmap`,
/// `munmap`, `mprotect`, and RSS accounting all work in. Region pages never go
/// below this: a sub-page "page" would still cost a full OS page (no RSS win),
/// and would break per-region `munmap` and the guardfree `mprotect` (both
/// OS-page-granular). Size classes are powers-of-two multiples of it.
pub(crate) const BASE_PAGE: usize = 4096;

/// `log2(BASE_PAGE)` — the size-class shift (class 0 == `BASE_PAGE`).
const BASE_PAGE_BITS: u32 = BASE_PAGE.trailing_zeros();

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
        debug_assert!(len >= BASE_PAGE && len.is_power_of_two());
        if len == BASE_PAGE {
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

    /// Diagnostic (`--trace=guardfree`): make the page inaccessible and leak
    /// the mapping so the address is never reused. A use-after-free then
    /// faults (SIGSEGV) at the exact dereference instead of silently reading
    /// a recycled slot — pinpointing the *use* site to pair with the
    /// free-log's *free* site. Run under gdb to read the backtrace.
    fn guard_and_leak(self) {
        unsafe {
            libc::mprotect(self.ptr as *mut libc::c_void, self.len, libc::PROT_NONE);
        }
        std::mem::forget(self); // keep the mapping reserved + inaccessible
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
    debug_assert!(size >= BASE_PAGE && size.is_power_of_two());
    (size.trailing_zeros() - BASE_PAGE_BITS) as usize // class 0 == BASE_PAGE
}

#[allow(dead_code)]
fn class_size(class: usize) -> usize {
    BASE_PAGE << class
}

/// Number of size classes to track (4K through 4MB = 11 classes).
const NUM_CLASSES: usize = 11;

// ── Page-claim histogram (a `--stats` exit summary, for page-size analysis) ──
//
// Under `--stats`, every `claim` records its (power-of-two) size into a global
// per-size-class histogram, printed at process exit alongside the other
// `[stats]` lines. It measures how often geometric growth escalates past
// `BASE_PAGE` across a run — the precondition for the large-page
// region-attribution cost (a large page is the only place `region_of_ptr`'s
// sub-alignment search can be fooled; see docs/impl/region/diagnostics.md).
// Off by default and zero-cost then: the per-claim path is a single config-flag
// read, so production pays nothing.
use std::sync::atomic::{AtomicU64, Ordering};

static PAGE_CLAIM_COUNT: [AtomicU64; NUM_CLASSES + 1] =
    [const { AtomicU64::new(0) }; NUM_CLASSES + 1];
static PAGE_CLAIM_BYTES: [AtomicU64; NUM_CLASSES + 1] =
    [const { AtomicU64::new(0) }; NUM_CLASSES + 1];

/// Whether `--stats` is active (the histogram's gate). Off ⇒ `record_claim` and
/// `dump_page_hist` are no-ops, so the histogram costs nothing in normal runs.
/// Read live from the global config like `has_trace`; config is installed before
/// any region (and therefore any page) is created.
fn page_hist_enabled() -> bool {
    crate::config::get().stats
}

fn record_claim(size: usize) {
    if !page_hist_enabled() {
        return;
    }
    // Register the at-exit dump on first recorded claim, so it fires even when
    // the test runner ends via `os/exit` (which runs C atexit handlers but not
    // Rust destructors, so the main-thread `--stats` block is bypassed).
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        extern "C" fn at_exit() {
            dump_page_hist();
        }
        libc::atexit(at_exit);
    });
    let bucket = size_class(size).min(NUM_CLASSES);
    PAGE_CLAIM_COUNT[bucket].fetch_add(1, Ordering::Relaxed);
    PAGE_CLAIM_BYTES[bucket].fetch_add(size as u64, Ordering::Relaxed);
}

/// Print the page-claim histogram to stderr under `--stats`: one `[stats]
/// page-claim size=<bytes> claims=<n> bytes=<n>` line per non-empty size class
/// (`size=0` = the oversized one-off bucket above the size classes). The fields
/// are stable so a batched corpus run can sum them across processes.
pub(crate) fn dump_page_hist() {
    if !page_hist_enabled() {
        return;
    }
    for c in 0..=NUM_CLASSES {
        let n = PAGE_CLAIM_COUNT[c].load(Ordering::Relaxed);
        if n == 0 {
            continue;
        }
        let bytes = PAGE_CLAIM_BYTES[c].load(Ordering::Relaxed);
        let size = if c < NUM_CLASSES { BASE_PAGE << c } else { 0 };
        eprintln!("[stats] page-claim size={size} claims={n} bytes={bytes}");
    }
}

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
        let size = size.next_power_of_two().max(BASE_PAGE);
        record_claim(size);
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
        if crate::value::fiberheap::freelog::guard_armed() {
            page.guard_and_leak();
            return;
        }
        let size = page.len();
        let class = size_class(size.next_power_of_two().max(BASE_PAGE));
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
        Self::new(BASE_PAGE, 4 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests;
