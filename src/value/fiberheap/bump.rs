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
//! # Pointer stability
//!
//! Each page is `Box<[MaybeUninit<u8>; PAGE_SIZE]>` — heap-allocated and
//! fixed-address. The outer `Vec<Box<...>>` stores page pointers; when the
//! Vec grows, only the pointer array reallocates, not the pages themselves.

use std::mem::{align_of, size_of, MaybeUninit};

use crate::value::heap::HeapObject;

/// Bytes per page. 64KB is large enough for typical working sets and
/// bounded so oversize allocations use fallback pages.
const PAGE_SIZE: usize = 64 * 1024;

/// Opaque position snapshot within a `BumpArena`.
#[derive(Clone, Copy)]
pub(crate) struct BumpMark {
    page: usize,
    offset: usize,
    alloc_count: usize,
}

/// Byte-level bump arena.
///
/// Invariant: `current_page` is always a valid index into `pages`
/// (or `pages` is empty and both page/offset are 0).
pub(crate) struct BumpArena {
    /// Owned pages of raw bytes.
    pages: Vec<Box<[MaybeUninit<u8>]>>,
    /// Current page index. 0 when pages is empty.
    current_page: usize,
    /// Next byte offset within the current page.
    offset: usize,
    /// Running count of `alloc()` calls for `HeapObject` (not slice allocs).
    alloc_count: usize,
}

impl BumpArena {
    pub fn new() -> Self {
        BumpArena {
            pages: Vec::new(),
            current_page: 0,
            offset: 0,
            alloc_count: 0,
        }
    }

    /// Raw byte allocation with alignment.
    ///
    /// Advances to a new page if the current page lacks space.
    /// Returns a pointer to uninitialized bytes.
    pub fn alloc_raw(&mut self, size: usize, align: usize) -> *mut u8 {
        if self.pages.is_empty() {
            self.add_page();
            self.current_page = 0;
            self.offset = 0;
        }

        // Align current offset up to the required alignment.
        let aligned = (self.offset + align - 1) & !(align - 1);
        if aligned + size > PAGE_SIZE {
            // Not enough space in current page — advance.
            self.current_page += 1;
            if self.current_page >= self.pages.len() {
                self.add_page();
            }
            self.offset = 0;
            // Re-align within the new page (always 0, which is page-aligned
            // since pages are at least 8-byte aligned via Box allocation).
        } else {
            self.offset = aligned;
        }

        let page = &mut self.pages[self.current_page];
        let ptr = unsafe { page.as_mut_ptr().add(self.offset) as *mut u8 };
        self.offset += size;
        ptr
    }

    /// Allocate a `HeapObject` into the arena, return a pointer.
    ///
    /// The pointer remains valid until `release_to()` truncates past it
    /// or `clear()` is called.
    pub fn alloc(&mut self, obj: HeapObject) -> *mut HeapObject {
        let ptr =
            self.alloc_raw(size_of::<HeapObject>(), align_of::<HeapObject>()) as *mut HeapObject;
        unsafe {
            std::ptr::write(ptr, obj);
        }
        self.alloc_count += 1;
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
            alloc_count: self.alloc_count,
        }
    }

    /// Reset the arena to the mark, freeing later pages.
    ///
    /// # Safety
    /// The caller is responsible for running destructors on any objects
    /// allocated after the mark before calling this — the arena does not
    /// track which objects need Drop.
    pub fn release_to(&mut self, mark: BumpMark) {
        // Drop pages after the mark's page.
        if !self.pages.is_empty() {
            self.pages.truncate(mark.page + 1);
        }
        self.current_page = mark.page;
        self.offset = mark.offset;
        self.alloc_count = mark.alloc_count;
    }

    /// Reset the arena entirely, keeping one page for reuse.
    ///
    /// # Safety
    /// The caller is responsible for running destructors on all live objects.
    pub fn clear(&mut self) {
        self.pages.truncate(1);
        self.current_page = 0;
        self.offset = 0;
        self.alloc_count = 0;
    }

    /// Total bytes allocated (across all pages).
    pub fn allocated_bytes(&self) -> usize {
        self.pages.len() * PAGE_SIZE
    }

    /// Check if a pointer falls within any of this arena's pages.
    ///
    /// Used by the outbox safety net to detect pointers into private heap.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as usize;
        for page in &self.pages {
            let base = page.as_ptr() as usize;
            let end = base + PAGE_SIZE;
            if addr >= base && addr < end {
                return true;
            }
        }
        false
    }

    /// Running `HeapObject` allocation count (never decremented; rotation metrics).
    #[allow(dead_code)]
    pub fn alloc_count(&self) -> usize {
        self.alloc_count
    }

    fn add_page(&mut self) {
        let page: Box<[MaybeUninit<u8>]> = std::iter::repeat_with(MaybeUninit::uninit)
            .take(PAGE_SIZE)
            .collect::<Vec<_>>()
            .into_boxed_slice();
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
        // MaybeUninit slots do not call HeapObject::drop.
        // Caller must have run dtors before dropping the arena.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{Cons, HeapObject};
    use crate::value::Value;

    fn cons_obj() -> HeapObject {
        HeapObject::Cons(Cons::new(Value::NIL, Value::NIL))
    }

    #[test]
    fn test_alloc_basic() {
        let mut arena = BumpArena::new();
        let ptr = arena.alloc(cons_obj());
        assert!(!ptr.is_null());
        assert_eq!(arena.alloc_count(), 1);
    }

    #[test]
    fn test_alloc_multiple_distinct() {
        let mut arena = BumpArena::new();
        let mut ptrs = vec![];
        for _ in 0..5 {
            ptrs.push(arena.alloc(cons_obj()));
        }
        assert_eq!(arena.alloc_count(), 5);
        let unique: std::collections::HashSet<usize> = ptrs.iter().map(|p| *p as usize).collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn test_pointer_stability_across_pages() {
        let mut arena = BumpArena::new();
        // Enough HeapObjects to force page growth (HeapObject is ~72 bytes,
        // so 1000+ allocations span multiple 64KB pages).
        let n = 2000usize;
        let mut ptrs = vec![];
        for i in 0u32..(n as u32) {
            let ptr = arena.alloc(HeapObject::Cons(Cons::new(
                Value::int(i as i64),
                Value::NIL,
            )));
            ptrs.push((ptr, i as i64));
        }
        assert_eq!(arena.alloc_count(), n);
        for (ptr, expected) in &ptrs {
            let obj = unsafe { &**ptr };
            match obj {
                HeapObject::Cons(c) => assert_eq!(c.first.as_int().unwrap(), *expected),
                _ => panic!("unexpected variant"),
            }
        }
    }

    #[test]
    fn test_mark_and_release() {
        let mut arena = BumpArena::new();
        arena.alloc(cons_obj());
        arena.alloc(cons_obj());
        let mark = arena.mark();
        for _ in 0..3000 {
            arena.alloc(cons_obj());
        }
        assert!(arena.alloc_count() > 2);
        assert!(arena.pages.len() > 1);
        arena.release_to(mark);
        assert_eq!(arena.alloc_count(), 2);
        assert_eq!(arena.pages.len(), 1);
    }

    #[test]
    fn test_clear_keeps_one_page() {
        let mut arena = BumpArena::new();
        for _ in 0..3000 {
            arena.alloc(cons_obj());
        }
        assert!(arena.pages.len() >= 3);
        arena.clear();
        assert_eq!(arena.alloc_count(), 0);
        assert_eq!(arena.pages.len(), 1);
        let p = arena.alloc(cons_obj());
        assert!(!p.is_null());
    }

    #[test]
    fn test_owns() {
        let mut arena = BumpArena::new();
        let ptr = arena.alloc(cons_obj()) as *const ();
        assert!(arena.owns(ptr));
        let x: i64 = 42;
        assert!(!arena.owns(&x as *const _ as *const ()));
    }

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
}
