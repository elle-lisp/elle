//! Per-region storage with dual-ended page layout.
//!
//! Each region owns its pages exclusively. HeapObject slots bump up from
//! the bottom (after a 16-byte header), inline data bumps down from the
//! top. When the two cursors meet, a new page is claimed from the PagePool.
//!
//! ```text
//! low addr                                          high addr
//! ┌──────────────────────────────────────────────────────┐
//! │ header │ HeapObj │ HeapObj │ ... │ free │ ... │ data │
//! │ (16B)  │  (48B)  │  (48B)  │     │      │     │bytes │
//! └──────────────────────────────────────────────────────┘
//!          ↑ obj_cursor bumps →          ← data_cursor ↑
//! ```
//!
//! Page header stores `region_id: u32` at offset 0, enabling O(1)
//! `region_of_ptr()`: round down to page alignment, read header. It also
//! stores the region's `(generation, store)` stamp (docs/impl/region/generations.md
//! § "Region generations"), letting debug builds detect a deref through a
//! freed-but-cached page.
//!
//! No per-slot free list — regions are freed in bulk only.

use std::mem::{align_of, size_of};

use super::pagepool::{MmapPage, PagePool};
use super::{holds_value_refs, needs_drop};
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

/// Page header at offset 0 of every region page.
/// Must be exactly `HEADER_SIZE` bytes.
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
    /// [`size_tag`]). `region_of_page_ptr` finds a variable-sized page's base by
    /// masking a pointer down to each candidate power-of-2 alignment and reading
    /// this field; the alignment whose tag matches is the true base. The 24-bit
    /// magic is what makes that search sound: a smaller sub-alignment of a LARGE
    /// page lands mid-page, on object/inline *data*, and a bare `log2` byte there
    /// can coincidentally equal the smaller size's log2 — a false base read as a
    /// garbage `(region_id, stamp)` (the `oracle.lisp` 584 GB `ensure_raw`
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

/// The [`PageHeader::size_tag`] value for a page of exactly `size` bytes:
/// the magic in the high 24 bits, `log2(size)` in the low 8. `size` is a
/// power of two, so `trailing_zeros()` is its log2 and always `< 256`.
#[inline]
fn size_tag(size: usize) -> u32 {
    debug_assert!(size.is_power_of_two());
    (PAGE_MAGIC << 8) | size.trailing_zeros()
}

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

const HEADER_SIZE: usize = size_of::<PageHeader>();
const _: () = assert!(HEADER_SIZE == 16);

/// Upper bound on a region's geometrically-growing page size.
///
/// A region doubles its next page size each time it claims one, so a
/// region that keeps allocating amortises page-claim cost. Left uncapped
/// the doubling races into hundreds of MB / multi-GB pages: a region
/// accumulating garbage (the s11 region model frees little until the
/// region dies) then claims a page double the last for the next handful
/// of bytes, turning a linear byte total into a geometric memory blowup —
/// `(apply concat …)` of small chunks reached a >1GB page and tripped the
/// `region_of_page_ptr` lookup. Saturating the growth here bounds the
/// over-allocation per region to one page. A single allocation larger
/// than this still gets a one-off page sized to fit (see `alloc_data`).
const MAX_PAGE_SIZE: usize = 1 << 22; // 4 MiB

/// A single page owned by a region.
struct RegionPage {
    page: MmapPage,
    /// Next free byte offset for objects (grows up from HEADER_SIZE).
    obj_cursor: usize,
    /// Next free byte offset for data (grows down from page top).
    data_cursor: usize,
}

impl RegionPage {
    fn new(mut page: MmapPage, region_id: u32, stamp: PageStamp) -> Self {
        let len = page.len();
        // Write the page header.
        unsafe {
            let header = page.as_mut_ptr() as *mut PageHeader;
            (*header).region_id = region_id;
            (*header).stamp = stamp;
            (*header).size_tag = size_tag(len);
        }
        RegionPage {
            page,
            obj_cursor: HEADER_SIZE,
            data_cursor: len,
        }
    }

    #[inline]
    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        self.data_cursor.saturating_sub(self.obj_cursor)
    }

    /// Check if there's space for a HeapObject and return the write pointer.
    /// Does NOT write — caller writes after confirming space.
    fn try_alloc_obj(&mut self) -> Option<*mut HeapObject> {
        let size = size_of::<HeapObject>();
        let align = align_of::<HeapObject>();
        let aligned = (self.obj_cursor + align - 1) & !(align - 1);
        if aligned + size > self.data_cursor {
            return None;
        }
        let ptr = unsafe { self.page.as_mut_ptr().add(aligned) as *mut HeapObject };
        self.obj_cursor = aligned + size;
        Some(ptr)
    }

    /// Try to allocate `size` bytes of data with `align` (bumps down).
    /// Returns pointer to the data, or None if no space.
    fn alloc_data(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // Align down: find the largest aligned address <= data_cursor - size.
        let end = self.data_cursor;
        if size > end {
            return None;
        }
        let start = (end - size) & !(align - 1);
        if start < self.obj_cursor {
            return None;
        }
        self.data_cursor = start;
        Some(unsafe { self.page.as_mut_ptr().add(start) })
    }

    /// Check if a pointer falls within this page.
    fn contains(&self, ptr: *const u8) -> bool {
        let base = self.page.as_ptr() as usize;
        let addr = ptr as usize;
        addr >= base && addr < base + self.page.len()
    }
}

/// Per-region storage: owns pages, tracks objects needing Drop.
pub(crate) struct RegionPool {
    pages: Vec<RegionPage>,
    region_id: u32,
    /// The `(generation, store)` stamp written into every page header this
    /// pool claims (docs/impl/region/generations.md § "Region generations"). Fixed for the
    /// pool's lifetime: a region is created at one generation, by one
    /// store, and freed whole.
    stamp: PageStamp,
    /// Number of HeapObject slots allocated.
    obj_count: usize,
    /// Pointers to HeapObjects that need Drop (destructor tracking).
    dtors: Vec<*mut HeapObject>,
    /// Pointers to HeapObjects that hold Value refs but don't need Drop
    /// (Pair, Parameter). Tracked for cascade decref on region free.
    ref_objs: Vec<*mut HeapObject>,
    /// Next page size to claim (doubles each time — geometric growth).
    next_page_size: usize,
    /// This region's owning-instance trace cell (a clone of the heap's), read by
    /// the `PAGES` page-claim gate in [`add_page`](Self::add_page). Per-instance,
    /// never a process-global — a `--trace=pages` toggle in one instance cannot
    /// make another instance's page claims spam.
    trace: crate::config::TraceCell,
}

mod introspect;

impl RegionPool {
    pub fn new(
        region_id: u32,
        stamp: PageStamp,
        initial_page_size: usize,
        trace: crate::config::TraceCell,
    ) -> Self {
        RegionPool {
            pages: Vec::new(),
            region_id,
            stamp,
            obj_count: 0,
            dtors: Vec::new(),
            ref_objs: Vec::new(),
            next_page_size: initial_page_size,
            trace,
        }
    }

    /// Allocate a HeapObject into this region and return a Value.
    pub fn alloc_obj(&mut self, obj: HeapObject, pool: &mut PagePool) -> Value {
        let value_tag = obj.value_tag();
        let tag = obj.tag();
        let drop = needs_drop(tag);
        let ptr = self.alloc_obj_raw(obj, pool);
        // Verify the page header's region_id matches this pool's region.
        debug_assert_eq!(
            unsafe { region_of_page_ptr(ptr as *const (), self.pages.last().unwrap().page.len(),) },
            self.region_id,
            "alloc_obj: page header region mismatch (region {}, {} pages, {} bytes — \
             run with --trace=pages to see the growth)",
            self.region_id,
            self.pages.len(),
            self.allocated_bytes(),
        );
        if drop {
            self.dtors.push(ptr);
        } else if holds_value_refs(tag) {
            self.ref_objs.push(ptr);
        }
        self.obj_count += 1;
        Value::from_heap_ptr(ptr as *const (), value_tag)
    }

    /// Allocate a HeapObject slot, returning a raw pointer.
    ///
    /// `obj` is consumed: written into the page via `ptr::write`.
    fn alloc_obj_raw(&mut self, obj: HeapObject, pool: &mut PagePool) -> *mut HeapObject {
        // Try the last page first.
        if let Some(page) = self.pages.last_mut() {
            if let Some(ptr) = page.try_alloc_obj() {
                unsafe { std::ptr::write(ptr, obj) };
                return ptr;
            }
        }
        // Claim a new page and retry.
        self.add_page(pool);
        let ptr = self
            .pages
            .last_mut()
            .unwrap()
            .try_alloc_obj()
            .expect("fresh page too small for HeapObject");
        unsafe { std::ptr::write(ptr, obj) };
        ptr
    }

    /// Allocate raw bytes for inline data in this region.
    pub fn alloc_data(&mut self, size: usize, align: usize, pool: &mut PagePool) -> *mut u8 {
        if size == 0 {
            return std::ptr::NonNull::<u8>::dangling().as_ptr();
        }
        // Try the last page first.
        if let Some(page) = self.pages.last_mut() {
            if let Some(ptr) = page.alloc_data(size, align) {
                return ptr;
            }
        }
        // Need more space. Claim a page large enough for the data.
        let min_size = HEADER_SIZE + size + align;
        while self.next_page_size < min_size {
            self.next_page_size *= 2;
        }
        self.add_page(pool);
        self.pages
            .last_mut()
            .unwrap()
            .alloc_data(size, align)
            .expect("fresh page too small for data")
    }

    /// Allocate and copy a slice into this region's inline data area.
    pub fn alloc_region_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
        pool: &mut PagePool,
    ) -> RegionSlice<T> {
        if items.is_empty() {
            return RegionSlice::empty();
        }
        let size = std::mem::size_of_val(items);
        let align = align_of::<T>();
        let ptr = self.alloc_data(size, align, pool) as *mut T;
        unsafe {
            std::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
        }
        unsafe { RegionSlice::from_raw(ptr, items.len() as u32) }
    }

    /// Teardown: run destructors, return all pages to the pool.
    ///
    /// After this call, the RegionPool is empty and should not be used.
    pub fn teardown(&mut self, pool: &mut PagePool) -> usize {
        // Run destructors in reverse allocation order.
        for &ptr in self.dtors.iter().rev() {
            if !ptr.is_null() {
                unsafe { std::ptr::drop_in_place(ptr) };
            }
        }
        self.dtors.clear();
        self.ref_objs.clear();

        // Return all pages to the pool.
        let pages = std::mem::take(&mut self.pages);
        for rp in pages {
            pool.release(rp.page);
        }
        let freed = self.obj_count;
        self.obj_count = 0;
        freed
    }

    /// Add a new page from the pool.
    fn add_page(&mut self, pool: &mut PagePool) {
        let page = pool.claim(self.next_page_size);
        let claimed = page.len();
        self.pages
            .push(RegionPage::new(page, self.region_id, self.stamp));
        if self.trace.load(std::sync::atomic::Ordering::Relaxed) & crate::config::trace_bits::PAGES
            != 0
        {
            eprintln!(
                "[trace:pages] add_page region={} requested={} claimed={} page_count={} total_bytes={} obj_count={}",
                self.region_id,
                self.next_page_size,
                claimed,
                self.pages.len(),
                self.allocated_bytes(),
                self.obj_count,
            );
        }
        // Geometric growth: double for next claim, saturating at
        // MAX_PAGE_SIZE so an accumulating region can't race into
        // multi-GB pages. A single allocation larger than the cap still
        // gets a one-off page: `alloc_data` bumps `next_page_size` up to
        // fit it before claiming, and this line re-caps afterward.
        self.next_page_size = self.next_page_size.saturating_mul(2).min(MAX_PAGE_SIZE);
    }
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
    // Geometric page growth is capped at MAX_PAGE_SIZE, but a single
    // oversized allocation can still claim a one-off page larger than
    // that, so the loop is bounded by the address width (shift to zero)
    // rather than a fixed size — a fixed `1 << 30` cap returned 0 for any
    // page above 1 GiB.
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

#[cfg(test)]
mod tests;
