//! Bump arena allocator for `HeapObject` and inline-slice allocations.
//!
//! Byte-level bump allocator: pages of raw bytes with variable-size
//! allocations aligned to the caller's requirements. Supports both
//! fixed-size `HeapObject` slots and variable-size data slices for the
//! Phase 2 inline-type migration.
//!
//! Unlike `RootSlab`, there is no per-slot free list — individual `dealloc`
//! is unsupported. Memory is reclaimed only by `release_to(mark)` (scope exit)
//! or `clear()` (teardown). Tail-call rotation is handled by the outer layer.
//!
//! # OS-level memory return
//!
//! Pages are backed by `mmap` (POSIX) rather than the process heap. When
//! `release_to` or `clear` truncates pages, the OS reclaims the physical
//! memory immediately via `munmap`. Pages past the high-water mark never
//! linger in allocator caches. The single page retained by `clear()` gets
//! `madvise(MADV_DONTNEED)` so the kernel drops its physical frames while
//! keeping the virtual mapping for fast reuse.
//!
//! # Pointer stability
//!
//! Each page is an `mmap`'d region at a fixed virtual address. The outer
//! `Vec<MmapPage>` stores page metadata; when the Vec grows, only the
//! metadata array reallocates, not the pages themselves.

use std::mem::align_of;

/// Bytes per standard page. 64KB is large enough for typical working sets
/// and bounded so oversize allocations use fallback pages.
const PAGE_SIZE: usize = 64 * 1024;

// ── mmap page abstraction ────────────────────────────────────────────

/// A single page of virtual memory obtained from the OS via `mmap`.
///
/// `munmap` on `Drop` returns the memory to the OS immediately — it does
/// not go through the process allocator (mimalloc), so there is no caching
/// layer hoarding the pages.
struct MmapPage {
    ptr: *mut u8,
    len: usize,
}

impl MmapPage {
    /// Allocate `len` bytes of zero-initialized page-aligned memory.
    ///
    /// Returns `None` if `mmap` fails (out of virtual address space).
    fn new(len: usize) -> Option<Self> {
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

    /// Advise the kernel that the page's physical frames are no longer
    /// needed. The virtual mapping stays alive (no re-mmap cost on reuse)
    /// but the kernel reclaims the physical memory. Next access triggers
    /// a zero-page fault (MAP_ANONYMOUS guarantees zero-fill).
    fn discard_contents(&self) {
        unsafe {
            libc::madvise(self.ptr as *mut libc::c_void, self.len, libc::MADV_DONTNEED);
        }
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for MmapPage {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.len);
        }
    }
}

// SAFETY: MmapPage owns its virtual memory exclusively; the raw pointer
// is never shared across threads. BumpArena is used within a single
// FiberHeap, which is thread-local.
unsafe impl Send for MmapPage {}

// ── BumpMark ──────────────────────────────────────────────────────────

/// Opaque position snapshot within a `BumpArena`.
#[derive(Clone, Copy)]
pub(crate) struct BumpMark {
    page: usize,
    offset: usize,
}

// ── BumpArena ─────────────────────────────────────────────────────────

/// Byte-level bump arena backed by OS pages.
///
/// Invariant: `current_page` is always a valid index into `pages`
/// (or `pages` is empty and both page/offset are 0).
pub(crate) struct BumpArena {
    /// Owned mmap'd pages.
    pages: Vec<MmapPage>,
    /// Current page index. 0 when pages is empty.
    current_page: usize,
    /// Next byte offset within the current page.
    offset: usize,
}

impl BumpArena {
    pub fn new() -> Self {
        BumpArena {
            pages: Vec::new(),
            current_page: 0,
            offset: 0,
        }
    }

    /// Raw byte allocation with alignment.
    ///
    /// Advances to a new page if the current page lacks space.
    /// Returns a pointer to zero-initialized bytes.
    ///
    /// Allocations larger than `PAGE_SIZE` get a dedicated oversized
    /// page of exactly `size` bytes. Subsequent allocations resume in
    /// a fresh standard-sized page — the oversized page is not reused.
    pub fn alloc_raw(&mut self, size: usize, align: usize) -> *mut u8 {
        if self.pages.is_empty() {
            self.add_page(PAGE_SIZE);
            self.current_page = 0;
            self.offset = 0;
        }

        // Oversized allocations get a dedicated page of exactly `size`
        // bytes. Bypasses the standard page to avoid a buffer overflow
        // from `offset += size` running past PAGE_SIZE.
        if size > PAGE_SIZE {
            self.add_page(size);
            self.current_page = self.pages.len() - 1;
            let ptr = self.pages[self.current_page].as_mut_ptr();
            // Mark this page as fully consumed so the next alloc
            // advances to a new standard page.
            self.offset = self.pages[self.current_page].len();
            return ptr;
        }

        // Align current offset up to the required alignment.
        let aligned = (self.offset + align - 1) & !(align - 1);
        if aligned + size > PAGE_SIZE {
            // Not enough space in current page — advance.
            self.current_page += 1;
            if self.current_page >= self.pages.len() {
                self.add_page(PAGE_SIZE);
            }
            self.offset = 0;
        } else {
            self.offset = aligned;
        }

        let ptr = unsafe { self.pages[self.current_page].as_mut_ptr().add(self.offset) };
        self.offset += size;
        ptr
    }

    /// Allocate and copy a slice of `T` into the arena.
    ///
    /// `T` must be `Copy` so items can be memcpy'd. Returns a pointer
    /// to the first element in the arena.
    pub fn alloc_slice<T: Copy>(&mut self, items: &[T]) -> *mut T {
        let size = std::mem::size_of_val(items);
        if size == 0 {
            // Return a dangling-but-aligned pointer for zero-length slices.
            return std::ptr::NonNull::<T>::dangling().as_ptr();
        }
        let ptr = self.alloc_raw(size, align_of::<T>()) as *mut T;
        unsafe {
            std::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
        }
        ptr
    }

    /// Capture the current position for later release.
    pub fn mark(&self) -> BumpMark {
        BumpMark {
            page: self.current_page,
            offset: self.offset,
        }
    }

    /// Reset the arena to the mark, freeing later pages.
    ///
    /// Pages past the mark are `munmap`'d — the OS reclaims their physical
    /// memory immediately. No allocator caching, no RSS hoarding.
    ///
    /// # Safety
    /// The caller is responsible for running destructors on any objects
    /// allocated after the mark before calling this — the arena does not
    /// track which objects need Drop.
    pub fn release_to(&mut self, mark: BumpMark) {
        // Drop (munmap) pages after the mark's page.
        self.pages.truncate(mark.page + 1);
        self.current_page = mark.page;
        self.offset = mark.offset;
    }

    /// Reset the arena entirely, keeping one page for reuse.
    ///
    /// The retained page gets `madvise(MADV_DONTNEED)` so the kernel
    /// drops its physical frames while preserving the virtual mapping.
    /// Subsequent allocations fault in fresh zero pages on demand.
    ///
    /// # Safety
    /// The caller is responsible for running destructors on all live objects.
    pub fn clear(&mut self) {
        if self.pages.is_empty() {
            self.current_page = 0;
            self.offset = 0;
            return;
        }
        // Discard all pages except the first. munmap returns them to the OS.
        self.pages.truncate(1);
        // Tell the kernel we don't need the remaining page's contents.
        // Physical frames are reclaimed; virtual mapping stays alive.
        self.pages[0].discard_contents();
        self.current_page = 0;
        self.offset = 0;
    }

    /// Total bytes committed across all pages.
    ///
    /// Accurate for both standard (64KB) and oversized pages.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.iter().map(|p| p.len()).sum()
    }

    /// Check if a pointer falls within any of this arena's pages.
    ///
    /// Used by the outbox safety net to detect pointers into private heap.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as usize;
        for page in &self.pages {
            let base = page.as_ptr() as usize;
            let end = base + page.len();
            if addr >= base && addr < end {
                return true;
            }
        }
        false
    }

    fn add_page(&mut self, size: usize) {
        let page = MmapPage::new(size).expect("bump arena: mmap failed");
        self.pages.push(page);
    }
}

impl Default for BumpArena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BumpArena {
    fn drop(&mut self) {
        // MmapPage::drop calls munmap for each page.
        // Caller must have run dtors before dropping the arena.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_alloc_slice_basic() {
        let mut arena = BumpArena::new();
        let data = [1u8, 2, 3, 4, 5];
        let ptr = arena.alloc_slice(&data);
        let slice = unsafe { std::slice::from_raw_parts(ptr, data.len()) };
        assert_eq!(slice, &data[..]);
    }

    #[test]
    fn test_alloc_slice_value() {
        let mut arena = BumpArena::new();
        let vals = [Value::int(1), Value::int(2), Value::int(3)];
        let ptr = arena.alloc_slice(&vals);
        let slice = unsafe { std::slice::from_raw_parts(ptr, vals.len()) };
        assert_eq!(slice[0].as_int(), Some(1));
        assert_eq!(slice[1].as_int(), Some(2));
        assert_eq!(slice[2].as_int(), Some(3));
    }

    #[test]
    fn test_alloc_slice_empty() {
        let mut arena = BumpArena::new();
        let empty: &[u8] = &[];
        let ptr = arena.alloc_slice(empty);
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_mark_and_release() {
        let mut arena = BumpArena::new();
        let data = [1u8; 256];
        arena.alloc_slice(&data);
        arena.alloc_slice(&data);
        let mark = arena.mark();
        // Force page growth: 3000 × 256 bytes ≈ 750KB > one 64KB page.
        for _ in 0..3000 {
            arena.alloc_slice(&data);
        }
        assert!(arena.pages.len() > 1);
        arena.release_to(mark);
        assert_eq!(arena.pages.len(), 1);
    }

    #[test]
    fn test_clear_keeps_one_page() {
        let mut arena = BumpArena::new();
        let data = [1u8; 64];
        for _ in 0..3000 {
            arena.alloc_slice(&data);
        }
        assert!(arena.pages.len() >= 3);
        arena.clear();
        assert_eq!(arena.pages.len(), 1);
        let p = arena.alloc_slice(&[42u8]);
        assert!(!p.is_null());
    }

    #[test]
    fn test_owns() {
        let mut arena = BumpArena::new();
        let ptr = arena.alloc_slice(&[1u8, 2, 3]) as *const ();
        assert!(arena.owns(ptr));
        let x: i64 = 42;
        assert!(!arena.owns(&x as *const _ as *const ()));
    }

    #[test]
    fn test_allocated_bytes_accurate_for_oversized() {
        let mut arena = BumpArena::new();
        // One standard page.
        arena.alloc_slice(&[1u8]);
        let standard_bytes = arena.allocated_bytes();
        assert_eq!(standard_bytes, PAGE_SIZE);

        // One oversized allocation larger than PAGE_SIZE.
        let big_size = PAGE_SIZE * 2;
        let big_data: Vec<u8> = vec![0xAB; big_size];
        let ptr = arena.alloc_slice(&big_data);
        assert!(!ptr.is_null());

        let total = arena.allocated_bytes();
        assert_eq!(
            total,
            PAGE_SIZE + big_size,
            "allocated_bytes should be sum of page sizes, not count * PAGE_SIZE"
        );
    }
}
