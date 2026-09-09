// audited: 2026-09-08
//! Per-region storage with dual-ended page layout.
//!
//! docs/impl/region/model.md
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
//! The 16-byte page header — the region id, the `(generation, store)` stamp,
//! and the self-validating size tag — lives in `header.rs` beside the masked
//! walk that reads it from a pointer.
//!
//! No per-slot free list — regions are freed in bulk only.

use std::mem::{align_of, size_of};

use super::pagepool::{MmapPage, PageDirty, PagePool};
use super::{holds_value_refs, needs_drop};
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

mod header;

#[cfg(test)]
pub(crate) use header::header_of_page_ptr;
pub(crate) use header::{header_if_valid, region_of_page_ptr, PageStamp, HEADER_SIZE};

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
        unsafe { header::write(page.as_mut_ptr(), region_id, stamp, len) };
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

    /// Hand this page back to the pool along with the spans this region wrote —
    /// the only route a region page takes out of a region, so the cursors that
    /// describe it can never be read off a different page
    /// (docs/impl/region/model.md § "Page recycling").
    ///
    /// The object span starts *after* the header, so the page keeps the
    /// `(region_id, generation, store)` stamp written at claim while it waits
    /// in the cache. That stamp is what a pointer outliving this region finds,
    /// and what makes the generation mismatch a debug-build panic at the deref
    /// site instead of a plausible read (docs/impl/region/generations.md).
    fn release_into(self, pool: &mut PagePool) {
        let len = self.page.len();
        let dirty = PageDirty::new(HEADER_SIZE..self.obj_cursor, self.data_cursor..len);
        pool.release(self.page, dirty);
    }

    /// Check if a pointer falls within this page.
    fn contains(&self, ptr: *const u8) -> bool {
        let base = self.page.as_ptr() as usize;
        let addr = ptr as usize;
        addr >= base && addr < base + self.page.len()
    }
}

/// One page's byte layout, as [`RegionPool::page_layouts`] reports it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PageLayout {
    pub base: usize,
    pub len: usize,
    pub obj_cursor: usize,
    pub data_cursor: usize,
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

        // Return all pages to the pool, each with the spans it was filled to.
        let pages = std::mem::take(&mut self.pages);
        for rp in pages {
            rp.release_into(pool);
        }
        let freed = self.obj_count;
        self.obj_count = 0;
        freed
    }

    /// Byte layout of every page this region owns, in claim order. The image
    /// dumper reads these to copy page bytes and bound the meaningful spans
    /// (docs/impl/image.md § Dumping).
    pub(crate) fn page_layouts(&self) -> Vec<PageLayout> {
        self.pages
            .iter()
            .map(|p| PageLayout {
                base: p.page.as_ptr() as usize,
                len: p.page.len(),
                obj_cursor: p.obj_cursor,
                data_cursor: p.data_cursor,
            })
            .collect()
    }

    /// Adopt a hydrated image page (docs/impl/image.md § Hydration steps 3–5):
    /// stamp its header with this region's identity and install the cursors
    /// the image's page table recorded, so later allocation into this region
    /// and the release spans both see the true layout.
    pub(crate) fn adopt_hydrated_page(
        &mut self,
        page: MmapPage,
        obj_cursor: usize,
        data_cursor: usize,
    ) {
        debug_assert!(
            HEADER_SIZE <= obj_cursor && obj_cursor <= data_cursor && data_cursor <= page.len(),
            "hydrated page cursors out of order: {obj_cursor}..{data_cursor} in {}",
            page.len(),
        );
        let mut rp = RegionPage::new(page, self.region_id, self.stamp);
        rp.obj_cursor = obj_cursor;
        rp.data_cursor = data_cursor;
        self.pages.push(rp);
    }

    /// Rebuild the object bookkeeping from an image's object index
    /// (docs/impl/image.md § Hydration step 5): route each object to `dtors`
    /// or `ref_objs` by the same predicates the alloc path uses, and count it.
    pub(crate) fn install_object_index(
        &mut self,
        objects: &[(*mut HeapObject, crate::value::heap::HeapTag)],
    ) {
        for &(ptr, tag) in objects {
            if needs_drop(tag) {
                self.dtors.push(ptr);
            } else if holds_value_refs(tag) {
                self.ref_objs.push(ptr);
            }
        }
        self.obj_count += objects.len();
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

#[cfg(test)]
mod tests;
