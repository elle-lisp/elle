// audited: 2026-10-22
//! The `@string` and `@bytes` store: an extent of the container's region, read as a slice and edited by one splice.
//!
//! docs/impl/mutable.md

use std::cell::Cell;
use std::ops::Range;
use std::ptr::NonNull;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;

/// The fewest bytes a growth claims, so a store built empty does not claim a
/// new extent for each of its first few bytes.
const MIN_EXTENT: usize = 8;

/// The borrow count while a splice runs. A count above zero is that many live
/// read guards, which are a `RefCell`'s rules.
const WRITING: isize = -1;

/// The contents of an `@string` or of `@bytes`: the first `len` bytes of an
/// extent of `cap` bytes in the container's region. An `@string`'s bytes are
/// UTF-8, and the caller of a splice keeps them so.
pub struct ByteStore {
    ptr: Cell<NonNull<u8>>,
    len: Cell<usize>,
    cap: Cell<usize>,
    borrow: Cell<isize>,
}

/// A read of a byte store. A splice panics while one is live.
pub struct ByteRead<'a>(&'a ByteStore);

impl std::ops::Deref for ByteRead<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        // SAFETY: the first `len` bytes of the extent are written, the extent
        // lives as long as the container's region, and no splice runs while
        // this guard is live.
        unsafe { std::slice::from_raw_parts(self.0.ptr.get().as_ptr(), self.0.len.get()) }
    }
}

impl Drop for ByteRead<'_> {
    #[inline]
    fn drop(&mut self) {
        self.0.borrow.set(self.0.borrow.get() - 1);
    }
}

/// A running splice: holds the borrow count at `WRITING`, and gives it back
/// even when the splice panics.
struct Writing<'a>(&'a ByteStore);

impl<'a> Writing<'a> {
    fn begin(store: &'a ByteStore) -> Self {
        assert!(store.borrow.get() == 0, "already borrowed");
        store.borrow.set(WRITING);
        Writing(store)
    }
}

impl Drop for Writing<'_> {
    fn drop(&mut self) {
        self.0.borrow.set(0);
    }
}

impl ByteStore {
    /// A store with no bytes, which claims no extent.
    pub(in crate::value) fn empty() -> Self {
        ByteStore {
            ptr: Cell::new(NonNull::dangling()),
            len: Cell::new(0),
            cap: Cell::new(0),
            borrow: Cell::new(0),
        }
    }

    /// A store holding `bytes`, in an extent of exactly their length in
    /// `region` on `heap`. The container's shell must be born in `region` too.
    pub(in crate::value) fn new_in(
        heap: &mut FiberHeap,
        region: RuntimeRegion,
        bytes: &[u8],
    ) -> Self {
        let store = ByteStore::empty();
        if bytes.is_empty() {
            return store;
        }
        let ptr = heap.alloc_room_in_region::<u8>(bytes.len(), region);
        // SAFETY: the room is fresh, holds `bytes.len()` bytes, and is not
        // `bytes`, which the caller holds borrowed.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
        store
            .ptr
            .set(NonNull::new(ptr).expect("a region allocation is never null"));
        store.len.set(bytes.len());
        store.cap.set(bytes.len());
        store
    }

    /// Read the bytes. Panics while a splice is running.
    #[inline]
    pub fn read(&self) -> ByteRead<'_> {
        self.try_read().expect("already mutably borrowed")
    }

    /// Read the bytes, or `None` while a splice is running.
    #[inline]
    pub fn try_read(&self) -> Option<ByteRead<'_>> {
        let count = self.borrow.get();
        if count == WRITING {
            return None;
        }
        self.borrow.set(count + 1);
        Some(ByteRead(self))
    }

    /// Replace the bytes in `range` with `with`, and answer the bytes it
    /// removed. A result longer than the capacity calls `room` for an extent
    /// of at least twice the capacity, and moves the bytes there. Panics when
    /// `range` is out of bounds, and when a read guard is live.
    pub(in crate::value) fn splice(
        &self,
        range: Range<usize>,
        with: &[u8],
        room: impl FnOnce(usize) -> NonNull<u8>,
    ) -> Vec<u8> {
        let _writing = Writing::begin(self);
        let (len, cap) = (self.len.get(), self.cap.get());
        assert!(
            range.start <= range.end && range.end <= len,
            "splice range {range:?} is out of bounds for {len} bytes"
        );
        let base = self.ptr.get().as_ptr();
        let tail = len - range.end;
        let new_len = range.start + with.len() + tail;
        // SAFETY: the range lies within the first `len` bytes, which are
        // written. Every copy below stays within an extent's capacity: the old
        // one holds `cap >= new_len` bytes, and a fresh one `grown >= new_len`.
        // `with` is not the live extent, because no read guard is live, so
        // only the in-place tail move can overlap, and `copy` allows that.
        unsafe {
            let removed = std::slice::from_raw_parts(base.add(range.start), range.len()).to_vec();
            if new_len > cap {
                let grown = new_len.max(cap.saturating_mul(2)).max(MIN_EXTENT);
                let fresh = room(grown);
                let to = fresh.as_ptr();
                std::ptr::copy_nonoverlapping(base, to, range.start);
                std::ptr::copy_nonoverlapping(with.as_ptr(), to.add(range.start), with.len());
                std::ptr::copy_nonoverlapping(
                    base.add(range.end),
                    to.add(range.start + with.len()),
                    tail,
                );
                self.ptr.set(fresh);
                self.cap.set(grown);
            } else {
                if with.len() != range.len() {
                    std::ptr::copy(
                        base.add(range.end),
                        base.add(range.start + with.len()),
                        tail,
                    );
                }
                std::ptr::copy(with.as_ptr(), base.add(range.start), with.len());
            }
            self.len.set(new_len);
            removed
        }
    }
}
