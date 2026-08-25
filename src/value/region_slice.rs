//! `RegionSlice`: a `(ptr, len)` view into data owned by a region.
//!
//! Used by immutable collection types (LString, LArray, LStruct, LBytes,
//! LSet) and closure environments to store variable-length data contiguously
//! in the region's pages — usually adjacent to the containing HeapObject
//! header. Eliminates inner Rust-heap allocations for the immutable types —
//! no Drop needed when the region is reclaimed.
//!
//! # It is a borrowing handle, not a value — copies ALIAS the backing
//!
//! `RegionSlice` is `Copy`, and a copy duplicates only the `(ptr, len)` pair:
//! both copies point at the *same* backing data in the *same* region. The name
//! says where that data lives — **a region** — because that is the fact every
//! holder must respect: a `RegionSlice` reachable from an object in a
//! *different* region is a cross-region reference and must incref the backing's
//! region (see `find_object_cross_refs`). Sharing one without that edge frees
//! the backing out from under a live holder. The canonical trap is
//! `squelch`/`attune` (src/primitives/meta.rs), which build a new closure that
//! shares the source closure's env `RegionSlice` — its backing stays in the
//! source's region (the protect+squelch+nested-yield UAF, fixed by the Closure
//! arm of `find_object_cross_refs`). The second instance was `with-traits`
//! (src/primitives/traits.rs), whose metadata-only clone copied the `(ptr,
//! len)` pair for the slice-backed immutables — freed-page reads once the
//! source died (tests/elle/region-withtraits-slice-uaf.lisp). The rule for
//! clones: copy the payload into the clone's own region
//! (`arena::alloc_region_slice`); only the closure-env share pays the
//! explicit-edge price instead.
//!
//! # Lifetime
//!
//! The `ptr` is valid only while the region that allocated it is live.
//! Since a `RegionSlice` is embedded in a HeapObject allocated in that same
//! region, `teardown()` reclaims both atomically — UNLESS another region's
//! object shares the slice (above), in which case the backing region's RC
//! keeps it alive until the last cross-region holder is freed.
//!
//! # Zero-length slices
//!
//! Empty slices use a dangling-but-aligned pointer. `std::slice::from_raw_parts`
//! accepts this as long as `len == 0`. An empty slice has no backing page, so
//! it is never a cross-region reference.

use std::fmt;

/// An immutable `(ptr, len)` view into `T` data owned by a region.
///
/// `Copy`; a copy aliases the same region-backed data (see the module docs —
/// sharing one across regions is a cross-region reference).
///
/// `T: 'static` because we cast raw pointers and don't propagate lifetimes.
/// Callers must ensure the owning region outlives any use of the slice.
#[repr(C)]
pub struct RegionSlice<T: 'static> {
    ptr: *const T,
    len: u32,
}

impl<T: 'static> RegionSlice<T> {
    /// An empty RegionSlice with a dangling-but-aligned pointer.
    pub fn empty() -> Self {
        RegionSlice {
            ptr: std::ptr::NonNull::<T>::dangling().as_ptr(),
            len: 0,
        }
    }

    /// Construct from a raw pointer and length.
    ///
    /// # Safety
    /// `ptr` must be aligned and valid for reading `len` elements, or `len` must be 0.
    pub unsafe fn from_raw(ptr: *const T, len: u32) -> Self {
        RegionSlice { ptr, len }
    }

    /// Reconstruct a Rust slice. Safe given the crate-wide invariant that
    /// the arena outlives any held RegionSlice.
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

    /// Byte layout of the slice header itself, for the image layout probes
    /// (docs/impl/image.md § Fingerprint): the `ptr` and `len` field offsets
    /// and the `len` field's size. Lives here because the fields are private.
    pub(crate) fn header_layout() -> (usize, usize, usize) {
        let probe = Self::empty();
        (
            std::mem::offset_of!(Self, ptr),
            std::mem::offset_of!(Self, len),
            std::mem::size_of_val(&probe.len),
        )
    }
}

// Manual Clone/Copy: just copies the pointer and length.
// Written manually rather than derived because `T` is not required to be Clone
// or Copy — `RegionSlice` is still Copy regardless of T.
impl<T: 'static> Copy for RegionSlice<T> {}
impl<T: 'static> Clone for RegionSlice<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> std::ops::Deref for RegionSlice<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: 'static + PartialEq> PartialEq for RegionSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: 'static + Eq> Eq for RegionSlice<T> {}

impl<T: 'static + std::hash::Hash> std::hash::Hash for RegionSlice<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl<T: 'static + PartialOrd> PartialOrd for RegionSlice<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T: 'static + Ord> Ord for RegionSlice<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: 'static + fmt::Debug> fmt::Debug for RegionSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

// Enable IntoIterator for &RegionSlice<T>, so `for x in &slice` works.
impl<'a, T: 'static> IntoIterator for &'a RegionSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}
