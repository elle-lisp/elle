// audited: 2026-10-27
//! An extent's borrow count: its read guards and its one running write, which keep a `RefCell`'s rules.
//!
//! docs/impl/mutable.md

use super::Extent;

/// The borrow count while a write runs. A count above zero is that many live
/// read guards.
const WRITING: isize = -1;

/// A read of an extent. A write panics while one is live.
pub struct ExtentRead<'a, T>(&'a Extent<T>);

impl<T> std::ops::Deref for ExtentRead<'_, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        // SAFETY: the first `len` items of the extent are written, the extent
        // lives as long as the container's region, and no write runs while
        // this guard is live.
        unsafe { std::slice::from_raw_parts(self.0.ptr.get().as_ptr(), self.0.len.get()) }
    }
}

impl<T> Drop for ExtentRead<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.borrow.set(self.0.borrow.get() - 1);
    }
}

/// A running write: holds the borrow count at `WRITING`, and gives it back
/// even when the write panics.
pub(super) struct Writing<'a, T>(&'a Extent<T>);

impl<'a, T> Writing<'a, T> {
    pub(super) fn begin(extent: &'a Extent<T>) -> Self {
        assert!(extent.borrow.get() == 0, "already borrowed");
        extent.borrow.set(WRITING);
        Writing(extent)
    }
}

impl<T> Drop for Writing<'_, T> {
    fn drop(&mut self) {
        self.0.borrow.set(0);
    }
}

impl<T: Copy + 'static> Extent<T> {
    /// Read the items. Panics while a write is running.
    #[inline]
    pub fn read(&self) -> ExtentRead<'_, T> {
        self.try_read().expect("already mutably borrowed")
    }

    /// Read the items, or `None` while a write is running.
    #[inline]
    pub fn try_read(&self) -> Option<ExtentRead<'_, T>> {
        let count = self.borrow.get();
        if count == WRITING {
            return None;
        }
        self.borrow.set(count + 1);
        Some(ExtentRead(self))
    }

    /// Run `f` over the items in place. `f` can reorder them but cannot
    /// change how many there are. Panics when a read guard is live.
    pub(in crate::value) fn with_slice_mut<R>(&self, f: impl FnOnce(&mut [T]) -> R) -> R {
        let _writing = Writing::begin(self);
        // SAFETY: the first `len` items are written, and the write guard keeps
        // every other borrow of the extent out until `f` returns.
        f(unsafe { std::slice::from_raw_parts_mut(self.ptr.get().as_ptr(), self.len.get()) })
    }
}
