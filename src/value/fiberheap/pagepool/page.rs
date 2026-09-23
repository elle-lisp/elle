// audited: 2026-09-22
//! An mmap-backed region page: how it is mapped self-aligned, and the gauge of
//! bytes every pool holds from the OS.
//!
//! docs/impl/region/model.md

use super::{base_page, PageDirty};
use std::sync::atomic::{AtomicU64, Ordering};

/// The cut that turns an over-allocated mapping into a self-aligned page: the
/// aligned base to keep, and the two runs to give back.
pub(super) struct Trim {
    /// Base of the `len`-byte, `len`-aligned page to keep.
    pub(super) base: usize,
    /// Bytes below `base`, unmapped.
    pub(super) prefix: usize,
    /// Bytes above the page, unmapped.
    pub(super) suffix: usize,
}

impl Trim {
    /// The first `len`-aligned `len`-byte window inside `alloc` bytes at `raw`.
    ///
    /// All three pieces are whole OS pages: `len` is a power-of-two multiple of
    /// [`base_page()`], and `mmap` answers with an OS-page-aligned address.
    /// `munmap` refuses any other address.
    pub(super) fn new(raw: usize, alloc: usize, len: usize) -> Self {
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
    /// recycles such a page, because it maps a file rather than anonymous
    /// memory. Its release is this page's `Drop`: `munmap`. Anonymous pages
    /// (every other constructor) are `false`.
    pub(super) file_backed: bool,
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
    pub(super) fn new(len: usize) -> Option<Self> {
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
    pub(super) fn reset(&mut self, dirty: &PageDirty) -> usize {
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
    pub(super) fn guard_and_leak(self) {
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
