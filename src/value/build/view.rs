// audited: 2026-10-20
//! The view constructors: a string or bytes header over another value's payload, with no copy.
//!
//! docs/impl/values.md

use super::lbytes;
use crate::hir::region::RuntimeRegion;
use crate::primitives::traitregistry::default_traits_for;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::FiberHeap;
use crate::value::Value;

/// An `LBytes` header in `region` over `backing`, with no copy: a view
/// (docs/impl/values.md). `backing` is the payload of a string or bytes
/// resident in a region of `heap`. The alloc-time scan records that region as
/// a cross-region reference when it is not `region`, so the source outlives
/// the view.
#[inline]
pub(crate) fn bytes_view(heap: &mut FiberHeap, backing: &[u8], region: RuntimeRegion) -> Value {
    lbytes(heap, view_slice(backing), region)
}

/// An `LString` header in `region` over `backing`, a view as [`bytes_view`]
/// is.
#[inline]
pub(crate) fn string_view(heap: &mut FiberHeap, backing: &str, region: RuntimeRegion) -> Value {
    let traits = default_traits_for(heap, HeapTag::LString);
    heap.alloc_in_region(
        HeapObject::LString {
            s: view_slice(backing.as_bytes()),
            traits,
        },
        region,
    )
}

/// The slice header a view carries: the backing's own address, or no backing
/// at all for an empty one, whose dangling pointer no scan may resolve.
fn view_slice(backing: &[u8]) -> crate::value::region_slice::RegionSlice<u8> {
    if backing.is_empty() {
        return crate::value::region_slice::RegionSlice::empty();
    }
    debug_assert!(
        u32::try_from(backing.len()).is_ok(),
        "a view's length must fit the slice header"
    );
    // SAFETY: `backing` is readable for its length, and it lies in a region's
    // pages (see `bytes_view`), which the edge the alloc scan records keeps
    // alive for as long as the header is.
    unsafe {
        crate::value::region_slice::RegionSlice::from_raw(backing.as_ptr(), backing.len() as u32)
    }
}
