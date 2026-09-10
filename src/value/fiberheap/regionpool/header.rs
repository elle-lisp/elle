// audited: 2026-09-08
//! The 16 bytes at a region page's base, and the masked walk that finds them
//! from a pointer anywhere inside the page.
//!
//! docs/impl/region/generations.md
//! docs/impl/region/model.md
//!
//! A page is self-aligned, so its base is the pointer masked down by the
//! page's own size — but the size is not known at the pointer, which is why
//! the walk tries each power-of-two alignment and asks the candidate base
//! whether it holds a real header.

use std::mem::size_of;

/// Page header at offset 0 of every region page.
/// Must be exactly [`HEADER_SIZE`] bytes.
#[repr(C)]
struct PageHeader {
    region_id: u32,
    /// The claiming store's identity for this page (docs/impl/region/generations.md
    /// § "Region generations"): which `RegionStore` claimed it, at which
    /// generation of `region_id`. A debug-build `region_of` compares the
    /// generation against the store's current one — a mismatch is a deref
    /// through a freed-but-cached page, a stale-region UAF caught at the
    /// exact deref. The store id scopes that comparison: generations from
    /// two different stores are unrelated numbers.
    stamp: PageStamp,
    /// Self-validating size tag: `(PAGE_MAGIC << 8) | page_size_log2` (see
    /// [`size_tag`]). [`header_of_page_ptr`] finds a variable-sized page's base
    /// by masking a pointer down to each candidate power-of-2 alignment and
    /// reading this field; the alignment whose tag matches is the true base. The
    /// 24-bit magic is what makes that search sound: a smaller sub-alignment of a
    /// LARGE page lands mid-page, on object/inline *data*, and a bare `log2` byte
    /// there can coincidentally equal the smaller size's log2 — a false base read
    /// as a garbage `(region_id, stamp)` (the `oracle.lisp` 584 GB `ensure_raw`
    /// blowup). Requiring the full magic makes a mid-page false match ~`1/2^32`
    /// instead of ~`1/256`; the authoritative defence is the ownership-validated
    /// walk in `RegionStore::region_of_ptr`.
    size_tag: u32,
}

/// 24-bit magic occupying the high bits of [`PageHeader::size_tag`]. An
/// arbitrary, non-trivial constant — its only job is to be vanishingly unlikely
/// to appear in object/inline data at a page-aligned offset, so a mid-page
/// false header match is rejected. (`0xE11E` reads "ELLE".)
const PAGE_MAGIC: u32 = 0x00E1_1E5C;

/// Bytes at a page's base that belong to the pool, not to the region: the
/// object cursor starts here, and an image's object index may not place an
/// object below it.
pub(crate) const HEADER_SIZE: usize = size_of::<PageHeader>();
const _: () = assert!(HEADER_SIZE == 16);

/// The `(generation, store)` pair stamped into each claimed page's header
/// alongside the region id (docs/impl/region/generations.md § "Region generations") —
/// grouped so the two u32s can't be swapped at a call site.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageStamp {
    /// The region id's generation at the owning pool's creation.
    pub generation: u32,
    /// Process-unique id of the claiming `RegionStore`.
    pub store: u32,
}

/// The [`PageHeader::size_tag`] value for a page of exactly `size` bytes:
/// the magic in the high 24 bits, `log2(size)` in the low 8. `size` is a
/// power of two, so `trailing_zeros()` is its log2 and always `< 256`.
#[inline]
fn size_tag(size: usize) -> u32 {
    debug_assert!(size.is_power_of_two());
    (PAGE_MAGIC << 8) | size.trailing_zeros()
}

/// Stamp a claimed `len`-byte page with the identity of the region taking it.
///
/// # Safety
/// `base` must be the start of a writable mapping of at least `len` bytes,
/// and `len` a power of two — the size the tag will claim.
pub(crate) unsafe fn write(base: *mut u8, region_id: u32, stamp: PageStamp, len: usize) {
    let header = base as *mut PageHeader;
    (*header).region_id = region_id;
    (*header).stamp = stamp;
    (*header).size_tag = size_tag(len);
}

/// Read the region id and `(generation, store)` stamp from the page header
/// at a given pointer. Returns `(0, PageStamp::default())` when no header
/// self-validates (not a region page).
///
/// Tries progressively larger power-of-2 alignments starting from
/// `min_page_size` until [`header_if_valid`] accepts one — its [`size_tag`]
/// carries the [`PAGE_MAGIC`] and that alignment's log2. This handles
/// variable-sized pages from geometric growth. The magic is what stops a
/// smaller sub-alignment of a *large* page (whose masked base lands mid-page,
/// on object data) from being read as a false header — but it is only
/// probabilistic; the authoritative resolver is `RegionStore::region_of_ptr`,
/// which additionally requires the matched region to *own* the pointer. Callers
/// that have the store (the RC-decision funnel) use that; this magic-only form
/// serves the free-time cross-ref scan, where the `valid_region` filter screens
/// the result.
///
/// # Safety
/// `ptr` must point into a page allocated by a RegionPool with a valid
/// PageHeader at the self-aligned base.
pub(crate) unsafe fn header_of_page_ptr(ptr: *const (), min_page_size: usize) -> (u32, PageStamp) {
    debug_assert!(min_page_size.is_power_of_two());
    let addr = ptr as usize;
    let mut size = min_page_size;
    // Walk every power-of-2 alignment from the smallest candidate up.
    // Geometric page growth is capped, but a single oversized allocation can
    // still claim a one-off page larger than that cap, so the loop is bounded
    // by the address width (shift to zero) rather than a fixed size — a fixed
    // `1 << 30` cap returned 0 for any page above 1 GiB.
    while size != 0 {
        if let Some(header) = header_if_valid(addr, size) {
            return header;
        }
        size <<= 1;
    }
    (0, PageStamp::default())
}

/// The page header at `addr`'s `size`-aligned base, *iff* it self-validates as a
/// real header for a page of exactly `size` bytes — its [`size_tag`] carries the
/// [`PAGE_MAGIC`] and `log2(size)`. `None` when the bytes there are not such a
/// header: a smaller sub-alignment of a larger page (mid-page object/inline data
/// — the magic rejects it) or an unrelated page size. This is the single
/// candidate-base test shared by [`header_of_page_ptr`]'s magic-only walk and
/// `RegionStore::region_of_ptr`'s ownership-validated walk.
///
/// # Safety
/// Same contract as [`header_of_page_ptr`]: the masked base must be a readable
/// page-aligned address.
pub(crate) unsafe fn header_if_valid(addr: usize, size: usize) -> Option<(u32, PageStamp)> {
    let page_base = addr & !(size - 1);
    let header = page_base as *const PageHeader;
    ((*header).size_tag == size_tag(size)).then(|| ((*header).region_id, (*header).stamp))
}

/// Read just the region_id from the page header at a given pointer — the
/// generation-blind probe for paths that must not generation-check (the
/// free-time cascade scan; see docs/impl/region/generations.md § "Region generations").
///
/// # Safety
/// Same contract as [`header_of_page_ptr`].
pub(crate) unsafe fn region_of_page_ptr(ptr: *const (), min_page_size: usize) -> u32 {
    header_of_page_ptr(ptr, min_page_size).0
}
