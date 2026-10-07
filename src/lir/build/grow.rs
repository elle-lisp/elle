// audited: 2026-10-06
//! `RegionVec`: a slice that grows in the working region, and `LirArena`, the region it grows in.
//!
//! docs/impl/lir.md
//!
//! A region bumps its data down from the top of a page, so nothing can be
//! appended to an extent already written. A full `RegionVec` claims an extent
//! twice the size and copies into it, and the old extent stays dead until the
//! region is freed.

use std::ptr::NonNull;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::regionstore::RegionMint;
use crate::value::fiberheap::FiberHeap;

/// A heap and the working region on it. `Copy`, so every slice that grows in
/// the region can carry it.
#[derive(Clone, Copy)]
pub(crate) struct LirArena {
    heap: NonNull<FiberHeap>,
    mint: RegionMint,
}

impl LirArena {
    /// Mint a working region on `heap`. The caller frees it, through
    /// [`LirArena::free`] or by releasing the region itself.
    pub(crate) fn mint(heap: &mut FiberHeap) -> LirArena {
        let mint = heap.new_runtime_region_tracked();
        LirArena {
            heap: NonNull::from(heap),
            mint,
        }
    }

    /// The region this arena allocates in.
    pub(crate) fn region(&self) -> RuntimeRegion {
        self.mint.region()
    }

    /// The heap the region lives on.
    pub(crate) fn heap(&self) -> NonNull<FiberHeap> {
        self.heap
    }

    /// Free the region, and give its id back if nothing ever allocated in it.
    ///
    /// # Safety
    ///
    /// The heap must still be alive, and no slice grown in the region may be
    /// read afterwards.
    pub(crate) unsafe fn free(self) {
        let heap = unsafe { &mut *self.heap.as_ptr() };
        heap.decref_region_if_present(self.region());
        heap.recycle_unmaterialized_region(self.mint);
    }

    /// Room for `n` values of `T` in the region, aligned and unwritten.
    fn room<T: Copy + 'static>(&self, n: usize) -> NonNull<T> {
        // SAFETY: an arena is built from a live `&mut FiberHeap`, and the
        // builder that owns it frees the region before that borrow ends.
        let heap = unsafe { &mut *self.heap.as_ptr() };
        let ptr = heap.alloc_room_in_region::<T>(n, self.region());
        NonNull::new(ptr).expect("a region allocation is never null")
    }
}

/// A slice of `T` that grows in an arena's region: push, truncate, and insert
/// at an index, as a `Vec` does.
pub(crate) struct RegionVec<T: Copy + 'static> {
    arena: LirArena,
    ptr: NonNull<T>,
    len: usize,
    cap: usize,
}

impl<T: Copy + 'static> RegionVec<T> {
    /// An empty slice that claims nothing until its first push.
    pub(crate) fn new(arena: LirArena) -> Self {
        assert!(
            std::mem::size_of::<T>() > 0,
            "a RegionVec holds sized items"
        );
        RegionVec {
            arena,
            ptr: NonNull::dangling(),
            len: 0,
            cap: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` items of the extent were written, and the
        // extent lives until the arena's region is freed.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: as `as_slice`, and `&mut self` makes the borrow unique.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    pub(crate) fn push(&mut self, value: T) {
        if self.len == self.cap {
            self.grow(self.len + 1);
        }
        // SAFETY: `len < cap`, so the slot lies inside the extent.
        unsafe { self.ptr.as_ptr().add(self.len).write(value) };
        self.len += 1;
    }

    /// Append every item of `items`.
    pub(crate) fn extend_from_slice(&mut self, items: &[T]) {
        let len = self.len;
        self.insert_slice(len, items);
    }

    /// Drop everything from `len` on. The extent keeps its room.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    /// Insert `items` before index `at`, moving the tail up: what
    /// `Vec::splice(at..at, items)` does.
    pub(crate) fn insert_slice(&mut self, at: usize, items: &[T]) {
        assert!(at <= self.len, "insert at {at} past the end {}", self.len);
        let need = self.len + items.len();
        if need > self.cap {
            self.grow(need);
        }
        // SAFETY: the extent holds `need` slots, the tail moves within it, and
        // `items` cannot alias the extent: no caller holds a borrow of a slice
        // while handing it `&mut self`.
        unsafe {
            let base = self.ptr.as_ptr();
            std::ptr::copy(base.add(at), base.add(at + items.len()), self.len - at);
            std::ptr::copy_nonoverlapping(items.as_ptr(), base.add(at), items.len());
        }
        self.len = need;
    }

    /// Move into an extent of at least `need` slots: twice the old room, and
    /// four at the least, as `Vec` grows.
    #[cold]
    fn grow(&mut self, need: usize) {
        let cap = need.max(self.cap * 2).max(4);
        let ptr = self.arena.room::<T>(cap);
        // SAFETY: the new extent is fresh and holds `cap >= len` slots.
        unsafe { std::ptr::copy_nonoverlapping(self.ptr.as_ptr(), ptr.as_ptr(), self.len) };
        self.ptr = ptr;
        self.cap = cap;
    }
}
