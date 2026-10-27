// audited: 2026-10-27
//! An extent of a container's region: the items of a mutable store, read through a guard and edited in place.
//!
//! docs/impl/mutable.md

mod borrow;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::ops::Range;
use std::ptr::NonNull;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
pub use borrow::ExtentRead;
use borrow::Writing;

/// The fewest bytes a growth claims, and the fewest items, so a store built
/// empty does not claim a new extent for each of its first few items. The
/// floor stops at `MAX_FLOOR_BYTES`, so an extent of large items, a tree's
/// nodes, grows from one.
const MIN_EXTENT_BYTES: usize = 8;
const MIN_EXTENT_ITEMS: usize = 4;
const MAX_FLOOR_BYTES: usize = 256;

/// The first `len` items of an extent of `cap` items in the container's
/// region. The store owns nothing outside the region's pages. The borrow
/// count is its read guards, or a running write.
pub struct Extent<T> {
    ptr: Cell<NonNull<T>>,
    len: Cell<usize>,
    cap: Cell<usize>,
    borrow: Cell<isize>,
}

/// The room a write that never grows hands a splice.
pub(in crate::value) fn no_room<T>(_: usize) -> *mut T {
    unreachable!("a write that shrinks a store claims no room")
}

impl<T: Copy + 'static> Extent<T> {
    /// A store with no items, which claims no extent.
    pub(in crate::value) fn empty() -> Self {
        const { assert!(std::mem::size_of::<T>() > 0, "an extent holds sized items") };
        Extent {
            ptr: Cell::new(NonNull::dangling()),
            len: Cell::new(0),
            cap: Cell::new(0),
            borrow: Cell::new(0),
        }
    }

    /// A store holding `items`, in an extent of exactly their length that
    /// `room` claims. An empty store calls no `room`.
    pub(in crate::value) fn new(items: &[T], room: impl FnOnce(usize) -> *mut T) -> Self {
        let extent = Extent::empty();
        if items.is_empty() {
            return extent;
        }
        let ptr = claimed(room, items.len());
        // SAFETY: the room is fresh, holds `items.len()` items, and is not
        // `items`, which the caller holds borrowed.
        unsafe { std::ptr::copy_nonoverlapping(items.as_ptr(), ptr.as_ptr(), items.len()) };
        extent.ptr.set(ptr);
        extent.len.set(items.len());
        extent.cap.set(items.len());
        extent
    }

    /// A store holding `items`, in an extent of `region` on `heap`. The
    /// container's shell must be born in `region` too.
    pub fn new_in(heap: &mut FiberHeap, region: RuntimeRegion, items: &[T]) -> Self {
        Extent::new(items, |n| heap.alloc_room_in_region::<T>(n, region))
    }

    /// Replace the items in `range` with `with`, and answer what `take` makes
    /// of the items it removed. A result longer than the capacity calls `room`
    /// for an extent of at least twice the capacity, and moves the items
    /// there. Panics when `range` is out of bounds, and when a read guard is
    /// live.
    pub(in crate::value) fn splice<R>(
        &self,
        range: Range<usize>,
        with: &[T],
        room: impl FnOnce(usize) -> *mut T,
        take: impl FnOnce(&[T]) -> R,
    ) -> R {
        let _writing = Writing::begin(self);
        let (len, cap) = (self.len.get(), self.cap.get());
        assert!(
            range.start <= range.end && range.end <= len,
            "splice range {range:?} is out of bounds for {len} items"
        );
        let base = self.ptr.get().as_ptr();
        let tail = len - range.end;
        let new_len = range.start + with.len() + tail;
        // SAFETY: the range lies within the first `len` items, which are
        // written. Every copy below stays within an extent's capacity: the old
        // one holds `cap >= new_len` items, and a fresh one `grown >= new_len`.
        // `with` is not the live extent, because no read guard is live, so
        // only the in-place tail move can overlap, and `copy` allows that.
        unsafe {
            let removed = take(std::slice::from_raw_parts(
                base.add(range.start),
                range.len(),
            ));
            if new_len > cap {
                let grown = self.grown(new_len);
                let fresh = claimed(room, grown);
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

    /// Append `with`, growing through `room` as [`Self::splice`] does.
    pub(in crate::value) fn append(&self, with: &[T], room: impl FnOnce(usize) -> *mut T) {
        let end = self.len.get();
        self.splice(end..end, with, room, |_| ());
    }

    /// Append `n` items that `item` makes, each written where it lies in the
    /// extent, growing through `room` as [`Self::splice`] does.
    pub(in crate::value) fn append_with(
        &self,
        n: usize,
        room: impl FnOnce(usize) -> *mut T,
        mut item: impl FnMut() -> T,
    ) {
        let _writing = Writing::begin(self);
        let len = self.len.get();
        let new_len = len + n;
        // SAFETY: the first `len` items are written, and a fresh extent holds
        // `grown >= new_len` items, so every write lies within the capacity.
        unsafe {
            if new_len > self.cap.get() {
                let grown = self.grown(new_len);
                let fresh = claimed(room, grown);
                std::ptr::copy_nonoverlapping(self.ptr.get().as_ptr(), fresh.as_ptr(), len);
                self.ptr.set(fresh);
                self.cap.set(grown);
            }
            let base = self.ptr.get().as_ptr();
            for at in len..new_len {
                base.add(at).write(item());
            }
        }
        self.len.set(new_len);
    }

    /// The capacity a growth to `new_len` items claims: twice the capacity,
    /// or `new_len` when that is more, and never fewer items than the floor.
    fn grown(&self, new_len: usize) -> usize {
        let size = std::mem::size_of::<T>();
        let floor = (MIN_EXTENT_BYTES / size)
            .max(MIN_EXTENT_ITEMS)
            .min((MAX_FLOOR_BYTES / size).max(1));
        new_len.max(self.cap.get().saturating_mul(2)).max(floor)
    }

    /// Store `item` at `index` and answer the item it displaced. Panics when
    /// `index` is out of bounds, and when a read guard is live.
    pub(in crate::value) fn replace(&self, index: usize, item: T) -> T {
        self.with_slice_mut(|items| std::mem::replace(&mut items[index], item))
    }

    /// Remove the last item and answer it, or `None` when there is none.
    /// Panics when a read guard is live.
    pub(in crate::value) fn pop(&self) -> Option<T> {
        let _writing = Writing::begin(self);
        let len = self.len.get().checked_sub(1)?;
        self.len.set(len);
        // SAFETY: the item at `len` was the last of the written items.
        Some(unsafe { self.ptr.get().as_ptr().add(len).read() })
    }
}

/// The extent `room` claims for `n` items.
fn claimed<T>(room: impl FnOnce(usize) -> *mut T, n: usize) -> NonNull<T> {
    NonNull::new(room(n)).expect("a region allocation is never null")
}
