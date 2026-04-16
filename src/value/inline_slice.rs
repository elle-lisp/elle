//! Inline slice: (ptr, len) pointing to data in the same arena as the
//! containing HeapObject.
//!
//! Used by immutable collection types (LString, LArray, LStruct, LBytes,
//! LSet) to store their variable-length data contiguously with the
//! HeapObject header. Eliminates inner Rust-heap allocations for the
//! immutable types — no Drop needed when the arena is reset.
//!
//! # Lifetime
//!
//! The `ptr` is valid only while the arena that allocated it is live.
//! Since InlineSlice is always embedded in a HeapObject that is also
//! arena-allocated, `release_to(mark)` / `teardown()` reclaim both
//! atomically.
//!
//! # Zero-length slices
//!
//! Empty slices use a dangling-but-aligned pointer. `std::slice::from_raw_parts`
//! accepts this as long as `len == 0`.

use std::fmt;

/// An immutable slice of `T` stored inline in an arena.
///
/// `T: 'static` because we cast raw pointers and don't propagate lifetimes.
/// Callers must ensure the arena outlives any use of the slice.
#[repr(C)]
pub struct InlineSlice<T: 'static> {
    ptr: *const T,
    len: u32,
}

impl<T: 'static> InlineSlice<T> {
    /// An empty InlineSlice with a dangling-but-aligned pointer.
    pub fn empty() -> Self {
        InlineSlice {
            ptr: std::ptr::NonNull::<T>::dangling().as_ptr(),
            len: 0,
        }
    }

    /// Construct from a raw pointer and length.
    ///
    /// # Safety
    /// `ptr` must be aligned and valid for reading `len` elements, or `len` must be 0.
    pub unsafe fn from_raw(ptr: *const T, len: u32) -> Self {
        InlineSlice { ptr, len }
    }

    /// Reconstruct a Rust slice. Safe given the crate-wide invariant that
    /// the arena outlives any held InlineSlice.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) }
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

// Manual Clone/Copy: just copies the pointer and length.
impl<T: 'static> Clone for InlineSlice<T> {
    fn clone(&self) -> Self {
        InlineSlice {
            ptr: self.ptr,
            len: self.len,
        }
    }
}
impl<T: 'static> Copy for InlineSlice<T> {}

impl<T: 'static> std::ops::Deref for InlineSlice<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: 'static + PartialEq> PartialEq for InlineSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: 'static + Eq> Eq for InlineSlice<T> {}

impl<T: 'static + std::hash::Hash> std::hash::Hash for InlineSlice<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl<T: 'static + PartialOrd> PartialOrd for InlineSlice<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T: 'static + Ord> Ord for InlineSlice<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: 'static + fmt::Debug> fmt::Debug for InlineSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

// Enable IntoIterator for &InlineSlice<T>, so `for x in &slice` works.
impl<'a, T: 'static> IntoIterator for &'a InlineSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}
