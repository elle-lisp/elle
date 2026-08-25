//! Per-thread page cache for region allocation.
//!
//! `PagePool` caches mmap'd pages organized by size class. When a region
//! needs a new page, it claims one from the pool (or mmaps fresh). When a
//! region is freed, its pages are returned to the pool for reuse. Pages
//! exceeding the cache limit are munmapped immediately.
//!
//! A page moves between a region and the cache untouched, in both directions:
//! no system call, no byte read or written, because the claimant stamps the
//! header and writes every slot it hands out. Under `--trace=scrub` a release
//! additionally blanks the spans the dying region wrote (`PageDirty`), so a
//! read through a pointer that outlived its region detonates instead of
//! returning plausible bytes. See docs/impl/region/model.md § "Page recycling".

/// The smallest base page this runtime uses, and the floor
/// `--region-page-size` applies. No supported host reports an OS page below
/// this.
const MIN_BASE_PAGE: usize = 4096;

/// The OS base page size — the smallest region page and the unit that `mmap`,
/// `munmap`, `mprotect`, and RSS accounting all work in. Region pages never go
/// below this: a sub-page "page" would still cost a full OS page (no RSS win),
/// and would break per-region `munmap` and the guardfree `mprotect` (both
/// OS-page-granular). Size classes are powers-of-two multiples of it.
///
/// Asked of the OS once and cached. Both accounting gauges the pool keeps
/// (`cached_bytes` and [`mapped_bytes`]) count the length a page records, so
/// that length has to be the length the kernel charges — see
/// docs/impl/region/model.md § "The base page is the OS page".
pub(crate) fn base_page() -> usize {
    static BASE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BASE.get_or_init(|| derive_base_page(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }))
}

/// The base page for an OS-reported page size. An answer the size-class
/// arithmetic cannot use — a `sysconf` failure (`-1`), a page below
/// [`MIN_BASE_PAGE`], or a non-power-of-two — takes the floor instead.
fn derive_base_page(os_page: libc::c_long) -> usize {
    match usize::try_from(os_page) {
        Ok(n) if n >= MIN_BASE_PAGE && n.is_power_of_two() => n,
        _ => MIN_BASE_PAGE,
    }
}

/// What a dying region wrote into one of its pages: the object slots it filled,
/// bumping up, and the inline-data suffix it filled, bumping down. The gap
/// between them it never touched. This is what `--trace=scrub` zeroes, and
/// bounding the scrub to these spans is what makes it cost the region's own
/// footprint rather than a page (docs/impl/region/model.md § "Page recycling").
///
/// **The page header is not one of the spans.** A cached page keeps the
/// `(region, generation, store)` stamp of the region that died on it, so a
/// pointer that outlived that region resolves to a page whose generation no
/// longer matches — the debug-build panic at the exact deref site
/// (docs/impl/region/generations.md). Zeroing offset 0 would leave that
/// pointer with no header to find. The object span therefore starts after the
/// header, and only `RegionPage` in [`super::regionpool`] — the one type that
/// knows where the header ends and where each cursor stopped — builds one.
#[derive(Clone, Debug)]
pub(crate) struct PageDirty {
    /// Written object slots, from the first byte after the page header up to
    /// the object cursor.
    objects: std::ops::Range<usize>,
    /// Written inline data, from the data cursor to the end of the page.
    data: std::ops::Range<usize>,
}

impl PageDirty {
    /// The spans of a page whose object slots filled `objects` and whose inline
    /// data filled `data`. The two cursors grow toward each other, so the object
    /// span ends at or before the data span begins — the assertion that catches
    /// the two handed over the wrong way round.
    pub fn new(objects: std::ops::Range<usize>, data: std::ops::Range<usize>) -> Self {
        debug_assert!(
            objects.end <= data.start,
            "page spans crossed: objects {objects:?} overlap data {data:?} — \
             the two arguments are swapped",
        );
        PageDirty { objects, data }
    }

    /// Every byte of a `len`-byte page, header included — the spans for a page
    /// that never carried a region layout, so there is no stamp to preserve.
    #[cfg(test)]
    pub fn whole(len: usize) -> Self {
        PageDirty::new(0..len, len..len)
    }

    /// The two spans clamped to a `len`-byte page.
    fn spans(&self, len: usize) -> [std::ops::Range<usize>; 2] {
        let clamp = |r: &std::ops::Range<usize>| r.start.min(len)..r.end.min(len);
        [clamp(&self.objects), clamp(&self.data)]
    }
}

/// The cut that turns an over-allocated mapping into a self-aligned page: the
/// aligned base to keep, and the two runs to give back.
struct Trim {
    /// Base of the `len`-byte, `len`-aligned page to keep.
    base: usize,
    /// Bytes below `base`, unmapped.
    prefix: usize,
    /// Bytes above the page, unmapped.
    suffix: usize,
}

impl Trim {
    /// The first `len`-aligned `len`-byte window inside `alloc` bytes at `raw`.
    ///
    /// All three pieces are whole OS pages: `len` is a power-of-two multiple of
    /// [`base_page()`], and `mmap` answers with an OS-page-aligned address.
    /// `munmap` refuses any other address.
    fn new(raw: usize, alloc: usize, len: usize) -> Self {
        debug_assert!(len.is_power_of_two() && alloc >= 2 * len);
        let base = (raw + len - 1) & !(len - 1);
        let prefix = base - raw;
        Trim {
            base,
            prefix,
            suffix: alloc - prefix - len,
        }
    }
}

/// Give `len` bytes at `addr` back to the OS, and check the kernel took them.
///
/// # Safety
/// `addr` must name a mapping this process owns, of at least `len` bytes.
unsafe fn unmap(addr: usize, len: usize) {
    if len == 0 {
        return;
    }
    let rc = libc::munmap(addr as *mut libc::c_void, len);
    debug_assert_eq!(
        rc,
        0,
        "munmap({addr:#x}, {len}) failed: {}",
        std::io::Error::last_os_error(),
    );
}

/// An mmap-backed page of memory with a known size.
///
/// On Drop, the page is munmapped — the OS reclaims the physical memory
/// immediately with no allocator caching layer.
pub(crate) struct MmapPage {
    ptr: *mut u8,
    len: usize,
    /// A hydrated image page: a `MAP_PRIVATE` view of an image file
    /// (docs/impl/image.md § Hydration step 3). The pool neither caches nor
    /// recycles such a page — its body is image bytes, not the blank slate
    /// the claim contract hands out — so its release is this page's `Drop`:
    /// `munmap`. Anonymous pages (every other constructor) are `false`.
    file_backed: bool,
}

impl MmapPage {
    /// Allocate `len` bytes of zero-initialized, self-aligned memory.
    ///
    /// Self-aligned means the returned address is a multiple of `len`.
    /// This is required by `region_of_page_ptr`, which masks a pointer
    /// with `!(len - 1)` to find the page base.
    ///
    /// For a base page, `mmap` already answers with a page-aligned address.
    /// For larger pages we over-allocate 2× and trim (munmap prefix/suffix) to
    /// get a `len`-aligned sub-range.
    fn new(len: usize) -> Option<Self> {
        debug_assert!(len >= base_page() && len.is_power_of_two());
        if len == base_page() {
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
        let trim = Trim::new(raw as usize, alloc, len);
        unsafe {
            unmap(raw as usize, trim.prefix);
            unmap(trim.base + len, trim.suffix);
        }
        MAPPED_BYTES.fetch_add(len as u64, Ordering::Relaxed);
        Some(MmapPage {
            ptr: trim.base as *mut u8,
            len,
            file_backed: false,
        })
    }

    /// Raw mmap without alignment trimming (used for base pages).
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
            MAPPED_BYTES.fetch_add(len as u64, Ordering::Relaxed);
            Some(MmapPage {
                ptr: ptr as *mut u8,
                len,
                file_backed: false,
            })
        }
    }

    /// Wrap an already-established fixed mapping (a hydrated image page) as a
    /// file-backed page the region system owns. See [`MmapPage::file_backed`].
    ///
    /// # Safety
    /// `ptr` must be the base of a live `len`-byte private mapping, `len`-aligned
    /// (the masked-header walk requires self-alignment), owned by no other
    /// `MmapPage` — this takes over its `munmap`.
    pub(crate) unsafe fn from_fixed_mapping(ptr: *mut u8, len: usize) -> Self {
        debug_assert!(len >= base_page() && len.is_power_of_two());
        debug_assert_eq!(
            ptr as usize & (len - 1),
            0,
            "hydrated page not self-aligned"
        );
        MAPPED_BYTES.fetch_add(len as u64, Ordering::Relaxed);
        MmapPage {
            ptr,
            len,
            file_backed: true,
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

    /// Diagnostic (`--trace=scrub`): zero the spans `dirty` names, so a read
    /// through a pointer that outlived this page's region finds an all-zero
    /// slot and detonates at `arena::deref` instead of returning the dead
    /// region's bytes (docs/impl/region/model.md § "Page recycling"). Returns
    /// the bytes written.
    fn reset(&mut self, dirty: &PageDirty) -> usize {
        let mut written = 0;
        for span in dirty.spans(self.len) {
            let Some(count) = span.end.checked_sub(span.start) else {
                continue;
            };
            unsafe { std::ptr::write_bytes(self.ptr.add(span.start), 0, count) };
            written += count;
        }
        written
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
        unsafe { unmap(self.ptr as usize, self.len) };
        MAPPED_BYTES.fetch_sub(self.len as u64, Ordering::Relaxed);
    }
}

/// Bytes every region page pool in the process holds from the OS right now:
/// raised by each `mmap`, lowered by each `munmap`. Guarded pages
/// (`--trace=guardfree`) keep their mapping on purpose and stay counted.
///
/// Process-wide by design. `arena/page-claims` reads one heap's claims, so it
/// cannot see a heap another thread owns — and a worker thread's heap is
/// exactly the memory a program that spawns workers has to get back
/// (docs/threads.md § "A worker owns its heap and gives it back"). This is the
/// gauge that says whether it did.
pub fn mapped_bytes() -> u64 {
    MAPPED_BYTES.load(Ordering::Relaxed)
}

static MAPPED_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: MmapPage owns its virtual memory exclusively.
unsafe impl Send for MmapPage {}

/// Size class index for a given page size. Class 0 is [`base_page()`]; each
/// class above it doubles.
fn size_class(size: usize) -> usize {
    size_class_of(base_page(), size)
}

/// [`size_class`] against an explicit base — the ladder as a pure function, so
/// its arithmetic is checkable for a page size this host does not have.
pub(crate) fn size_class_of(base: usize, size: usize) -> usize {
    debug_assert!(size >= base && size.is_power_of_two());
    (size.trailing_zeros() - base.trailing_zeros()) as usize
}

/// The page size of class `class` on a ladder rooted at `base`.
pub(crate) fn class_size_of(base: usize, class: usize) -> usize {
    base << class
}

/// Number of size classes to track: the base page through `base << 10`.
const NUM_CLASSES: usize = 11;

// ── Page-claim histogram (a `--stats` exit summary, for page-size analysis) ──
//
// Under `--stats`, every `claim` records its (power-of-two) size into a global
// per-size-class histogram, printed at process exit alongside the other
// `[stats]` lines. It measures how often geometric growth escalates past
// `base_page()` across a run — the precondition for the large-page
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
        let size = if c < NUM_CLASSES {
            class_size_of(base_page(), c)
        } else {
            0
        };
        eprintln!("[stats] page-claim size={size} claims={n} bytes={bytes}");
    }
}

/// Live counters for one `PagePool` — the measurement surface behind the
/// `arena/page-claims` gauge (docs/impl/region/diagnostics.md) and behind the
/// contract tests for the claim path (docs/impl/region/model.md § "Page
/// recycling"). Both are monotonic and always on: a counter a release binary
/// does not keep is a counter a test cannot read.
#[derive(Default)]
pub(crate) struct PoolCounters {
    /// Pages handed out by `claim`, fresh mappings and recycled pages alike.
    claims: u64,
    /// Claims served from a free list instead of a fresh `mmap`.
    recycles: u64,
}

impl PoolCounters {
    /// Pages handed out, fresh and recycled alike.
    pub fn claims(&self) -> u64 {
        self.claims
    }

    /// Claims served from a free list.
    #[cfg(test)]
    pub fn recycles(&self) -> u64 {
        self.recycles
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
    /// Live traffic counters. See [`PoolCounters`].
    counters: PoolCounters,
}

impl PagePool {
    pub fn new(initial_page_size: usize, max_cached: usize) -> Self {
        PagePool {
            free_lists: (0..NUM_CLASSES).map(|_| Vec::new()).collect(),
            cached_bytes: 0,
            max_cached,
            initial_page_size,
            counters: PoolCounters::default(),
        }
    }

    /// This pool's live traffic counters.
    pub fn counters(&self) -> &PoolCounters {
        &self.counters
    }

    /// Initial page size for new regions.
    #[inline]
    pub fn initial_page_size(&self) -> usize {
        self.initial_page_size
    }

    /// Claim a page of exactly `size` bytes, its body zero.
    ///
    /// Pops from the free list if available (O(1)), otherwise mmaps fresh.
    /// Either way the body arrives blank, and this path does nothing to make it
    /// so: a fresh mapping is zero already, and a cached page was reset when its
    /// region released it (docs/impl/region/model.md § "Page recycling"). So a
    /// claim is a free-list pop — no system call, no page byte touched, and no
    /// fault on memory that is already resident. The caller stamps the header,
    /// which until then still carries the dead region's stamp.
    pub fn claim(&mut self, size: usize) -> MmapPage {
        let size = size.next_power_of_two().max(base_page());
        record_claim(size);
        self.counters.claims += 1;
        let class = size_class(size);
        if class < NUM_CLASSES {
            if let Some(page) = self.free_lists[class].pop() {
                self.cached_bytes -= page.len();
                self.counters.recycles += 1;
                // Nothing to do to the page: the caller stamps the header and
                // writes every slot it hands out.
                return page;
            }
        }
        MmapPage::new(size).expect("pagepool: mmap failed")
    }

    /// Return a single page to the cache, `dirty` naming the spans its region
    /// wrote.
    ///
    /// The page keeps its contents: the next claimant stamps the header and
    /// writes every slot it hands out, so blanking the body would be work with
    /// no reader (docs/impl/region/model.md § "Page recycling"). Under
    /// `--trace=scrub` the body is blanked anyway, over `dirty` only, so that a
    /// read through a pointer that outlived its region finds zeros and detonates
    /// at the deref instead of returning the dead region's bytes.
    ///
    /// A page the cache has no room for is munmapped, and is never scrubbed:
    /// `munmap` erases the work, and an unmapped address faults on its own.
    pub fn release(&mut self, mut page: MmapPage, dirty: PageDirty) {
        if crate::value::fiberheap::freelog::guard_armed() {
            page.guard_and_leak();
            return;
        }
        if page.file_backed {
            // A hydrated image page: never cached, never recycled — drop
            // unmaps it (see `MmapPage::file_backed`).
            return;
        }
        let size = page.len();
        let class = size_class(size.next_power_of_two().max(base_page()));
        if class < NUM_CLASSES && self.cached_bytes + size <= self.max_cached {
            self.cached_bytes += size;
            if crate::value::fiberheap::freelog::scrub_armed() {
                page.reset(&dirty);
            }
            self.free_lists[class].push(page);
        }
        // else: page is dropped here → munmap
    }

    /// Total cached bytes.
    pub fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }

    /// The page a `size`-class claim would recycle next, for the tests that
    /// read or mark a cached page's bytes behind the pool's back.
    #[cfg(test)]
    pub fn peek_cached(&mut self, size: usize) -> Option<&mut MmapPage> {
        self.free_lists
            .get_mut(size_class(size.next_power_of_two().max(base_page())))?
            .last_mut()
    }
}

impl Default for PagePool {
    fn default() -> Self {
        Self::new(base_page(), 4 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests;
