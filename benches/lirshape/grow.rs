// audited: 2026-10-06
//! `RegionVec`: a slice that grows in its own region, the prototype of the
//! working form a region-native lowerer would build in.
//!
//! docs/impl/image/measurements.md
//!
//! A region bumps its data down from the top of a page, so nothing can be
//! appended to an extent already written. A `RegionVec` that runs out of room
//! claims an extent twice the size and copies into it. The extent it leaves is
//! dead until the region is freed, and the arena counts those bytes, because
//! they are the cost this design pays where `Vec` pays a `realloc`.

use std::mem::{align_of, size_of};

use elle::hir::region::RuntimeRegion;
use elle::value::fiberheap::FiberHeap;

/// The region a build grows its slices in, and what the growth has cost.
pub struct Arena {
    heap: *mut FiberHeap,
    region: RuntimeRegion,
    /// Bytes of extents a slice grew out of and left behind.
    pub abandoned: usize,
    /// Extents claimed, growth and compaction alike.
    pub claims: usize,
}

impl Arena {
    pub fn new(heap: &mut FiberHeap, region: RuntimeRegion) -> Self {
        Arena {
            heap: heap as *mut FiberHeap,
            region,
            abandoned: 0,
            claims: 0,
        }
    }

    /// Room for `n` values of `T` in this arena's region, aligned for `T` and
    /// not yet written.
    ///
    /// The heap's public surface allocates bytes at alignment one, so this asks
    /// for `align - 1` bytes more and rounds the start up.
    pub fn alloc<T: Copy>(&mut self, n: usize) -> *mut T {
        if n == 0 {
            return std::ptr::NonNull::<T>::dangling().as_ptr();
        }
        self.claims += 1;
        let align = align_of::<T>();
        let bytes = n * size_of::<T>() + align - 1;
        let raw = unsafe { (*self.heap).alloc_bytes_in_region_with(bytes, self.region, |_| {}) };
        let addr = raw.as_ptr() as usize;
        ((addr + align - 1) & !(align - 1)) as *mut T
    }
}

/// A growable slice of `T` in an arena's region: `Vec`'s push, truncate and
/// splice, over region pages.
///
/// `Copy`, so a block's slice can sit inside another `RegionVec`. A copy
/// aliases the same extent, and the build keeps exactly one copy live.
pub struct RegionVec<T: Copy> {
    ptr: *mut T,
    len: u32,
    cap: u32,
    arena: *mut Arena,
}

impl<T: Copy> Clone for RegionVec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for RegionVec<T> {}

impl<T: Copy> RegionVec<T> {
    /// An empty slice that claims nothing until its first push.
    pub fn new(arena: *mut Arena) -> Self {
        RegionVec {
            ptr: std::ptr::NonNull::<T>::dangling().as_ptr(),
            len: 0,
            cap: 0,
            arena,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        if self.len == self.cap {
            self.grow(self.len as usize + 1);
        }
        unsafe { self.ptr.add(self.len as usize).write(value) };
        self.len += 1;
    }

    /// Drop everything from `len` on. The extent keeps its room.
    pub fn truncate(&mut self, len: usize) {
        if len < self.len as usize {
            self.len = len as u32;
        }
    }

    /// Insert `items` before index `at`, moving the tail up: what
    /// `Vec::splice(at..at, items)` does.
    pub fn insert_slice(&mut self, at: usize, items: &[T]) {
        let len = self.len as usize;
        assert!(at <= len, "insert at {at} past the end {len}");
        let need = len + items.len();
        if need > self.cap as usize {
            self.grow(need);
        }
        unsafe {
            std::ptr::copy(self.ptr.add(at), self.ptr.add(at + items.len()), len - at);
            std::ptr::copy_nonoverlapping(items.as_ptr(), self.ptr.add(at), items.len());
        }
        self.len = need as u32;
    }

    /// Move into an extent of at least `need` slots: twice the old room, and
    /// four at the least, as `Vec` grows.
    #[cold]
    fn grow(&mut self, need: usize) {
        let cap = need.max(self.cap as usize * 2).max(4);
        let arena = unsafe { &mut *self.arena };
        let ptr = arena.alloc::<T>(cap);
        unsafe { std::ptr::copy_nonoverlapping(self.ptr, ptr, self.len as usize) };
        arena.abandoned += self.cap as usize * size_of::<T>();
        self.ptr = ptr;
        self.cap = cap as u32;
    }
}
