//! Per-region storage with dual-ended page layout.
//!
//! Each region owns its pages exclusively. HeapObject slots bump up from
//! the bottom (after an 8-byte header), inline data bumps down from the
//! top. When the two cursors meet, a new page is claimed from the PagePool.
//!
//! ```text
//! low addr                                          high addr
//! ┌──────────────────────────────────────────────────────┐
//! │ header │ HeapObj │ HeapObj │ ... │ free │ ... │ data │
//! │  (8B)  │  (48B)  │  (48B)  │     │      │     │bytes │
//! └──────────────────────────────────────────────────────┘
//!          ↑ obj_cursor bumps →          ← data_cursor ↑
//! ```
//!
//! Page header stores `region_id: u16` at offset 0, enabling O(1)
//! `region_of_ptr()`: round down to page alignment, read header.
//!
//! No per-slot free list — regions are freed in bulk only.

use std::mem::{align_of, size_of};

use super::pagepool::{MmapPage, PagePool};
use super::{holds_value_refs, needs_drop};
use crate::value::heap::HeapObject;
use crate::value::inline_slice::InlineSlice;
use crate::value::Value;

/// Page header at offset 0 of every region page.
/// Must be exactly `HEADER_SIZE` bytes.
#[repr(C)]
struct PageHeader {
    region_id: u16,
    /// log2 of page size (e.g., 12 for 4096). Used by region_of_page_ptr
    /// to find the correct page base for variable-sized pages.
    page_size_log2: u8,
    _pad: [u8; 5],
}

const HEADER_SIZE: usize = size_of::<PageHeader>();
const _: () = assert!(HEADER_SIZE == 8);

/// A single page owned by a region.
struct RegionPage {
    page: MmapPage,
    /// Next free byte offset for objects (grows up from HEADER_SIZE).
    obj_cursor: usize,
    /// Next free byte offset for data (grows down from page top).
    data_cursor: usize,
}

impl RegionPage {
    fn new(mut page: MmapPage, region_id: u16) -> Self {
        let len = page.len();
        // Write the page header.
        unsafe {
            let header = page.as_mut_ptr() as *mut PageHeader;
            (*header).region_id = region_id;
            (*header).page_size_log2 = len.trailing_zeros() as u8;
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
    region_id: u16,
    /// Number of HeapObject slots allocated.
    obj_count: usize,
    /// Pointers to HeapObjects that need Drop (destructor tracking).
    dtors: Vec<*mut HeapObject>,
    /// Pointers to HeapObjects that hold Value refs but don't need Drop
    /// (Pair, Parameter). Tracked for cascade decref on region free.
    ref_objs: Vec<*mut HeapObject>,
    /// Next page size to claim (doubles each time — geometric growth).
    next_page_size: usize,
}

impl RegionPool {
    pub fn new(region_id: u16, initial_page_size: usize) -> Self {
        RegionPool {
            pages: Vec::new(),
            region_id,
            obj_count: 0,
            dtors: Vec::new(),
            ref_objs: Vec::new(),
            next_page_size: initial_page_size,
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
            "alloc_obj: page header region mismatch",
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
    pub fn alloc_inline_slice<T: Copy + 'static>(
        &mut self,
        items: &[T],
        pool: &mut PagePool,
    ) -> InlineSlice<T> {
        if items.is_empty() {
            return InlineSlice::empty();
        }
        let size = std::mem::size_of_val(items);
        let align = align_of::<T>();
        let ptr = self.alloc_data(size, align, pool) as *mut T;
        unsafe {
            std::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
        }
        unsafe { InlineSlice::from_raw(ptr, items.len() as u32) }
    }

    #[allow(dead_code)]
    pub fn obj_count(&self) -> usize {
        self.obj_count
    }

    /// Total committed bytes across all pages.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.iter().map(|p| p.page.len()).sum()
    }

    /// Check if a pointer falls within any of this region's pages.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as *const u8;
        self.pages.iter().any(|p| p.contains(addr))
    }

    /// Walk dtor objects and collect region IDs of cross-region references.
    ///
    /// Must be called BEFORE teardown (dtors are still alive).
    /// `own_id` is this region's ID — self-references are excluded.
    /// `page_size` is needed for `region_of_page_ptr`.
    /// Walk objects and collect region IDs of cross-region references.
    ///
    /// Must be called BEFORE teardown (dtors are still alive).
    /// `own_id` is this region's ID — self-references are excluded.
    /// `valid_region` filters out bogus region IDs from pointers not
    /// managed by the region store (e.g., shared heap values).
    pub fn collect_cross_region_refs(
        &self,
        own_id: u16,
        page_size: usize,
        valid_region: &dyn Fn(u16) -> bool,
    ) -> Vec<u16> {
        let mut refs = Vec::new();
        for &ptr in self.dtors.iter().chain(self.ref_objs.iter()) {
            if ptr.is_null() {
                continue;
            }
            let obj = unsafe { &*ptr };
            Self::collect_value_refs(obj, own_id, page_size, valid_region, &mut refs);
        }
        refs
    }

    /// Extract cross-region Value references from a HeapObject.
    pub(crate) fn collect_value_refs(
        obj: &HeapObject,
        own_id: u16,
        page_size: usize,
        valid_region: &dyn Fn(u16) -> bool,
        refs: &mut Vec<u16>,
    ) {
        let mut check = |val: &Value| {
            if !val.is_heap() {
                return;
            }
            if let Some(ptr) = val.as_heap_ptr() {
                let rid = unsafe { region_of_page_ptr(ptr, page_size) };
                if rid != 0 && rid != own_id && valid_region(rid) {
                    refs.push(rid);
                }
            }
        };

        match obj {
            HeapObject::LArrayMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for v in borrowed.iter() {
                        check(v);
                    }
                }
            }
            HeapObject::LStructMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for v in borrowed.values() {
                        check(v);
                    }
                }
            }
            HeapObject::LBox { cell, .. } | HeapObject::CaptureCell { cell, .. } => {
                if let Ok(borrowed) = cell.try_borrow() {
                    check(&borrowed);
                }
            }
            HeapObject::LSetMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    for v in borrowed.iter() {
                        check(v);
                    }
                }
            }
            HeapObject::Closure { closure, .. } => {
                for v in closure.env.iter() {
                    check(v);
                }
            }
            HeapObject::Pair(pair) => {
                check(&pair.first);
                check(&pair.rest);
            }
            HeapObject::LArray { elements, .. } => {
                for v in elements.iter() {
                    check(v);
                }
            }
            HeapObject::LStruct { data, .. } => {
                for (_, v) in data.iter() {
                    check(v);
                }
            }
            HeapObject::LSet { data, .. } => {
                for v in data.iter() {
                    check(v);
                }
            }
            HeapObject::Parameter { default, .. } => {
                check(default);
            }
            // Non-container types: no Value references to track.
            HeapObject::LString { .. }
            | HeapObject::LStringMut { .. }
            | HeapObject::LBytes { .. }
            | HeapObject::LBytesMut { .. }
            | HeapObject::Float(_)
            | HeapObject::NativeFn(_)
            | HeapObject::LibHandle(_)
            | HeapObject::ThreadHandle { .. }
            | HeapObject::Fiber { .. }
            | HeapObject::Syntax { .. }
            | HeapObject::FFISignature(_, _)
            | HeapObject::FFIType(_)
            | HeapObject::ManagedPointer { .. }
            | HeapObject::External { .. } => {}
        }
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
        self.pages.push(RegionPage::new(page, self.region_id));
        // Geometric growth: double for next claim.
        self.next_page_size = self.next_page_size.saturating_mul(2);
    }
}

/// Read the region_id from the page header at a given pointer.
///
/// Tries progressively larger power-of-2 alignments starting from
/// `min_page_size` until it finds a page header whose `page_size_log2`
/// matches the alignment. This handles variable-sized pages from
/// geometric growth.
///
/// # Safety
/// `ptr` must point into a page allocated by a RegionPool with a valid
/// PageHeader at the self-aligned base.
pub(crate) unsafe fn region_of_page_ptr(ptr: *const (), min_page_size: usize) -> u16 {
    debug_assert!(min_page_size.is_power_of_two());
    let addr = ptr as usize;
    let mut size = min_page_size;
    while size <= (1 << 30) {
        let page_base = addr & !(size - 1);
        let header = page_base as *const PageHeader;
        if (*header).page_size_log2 == size.trailing_zeros() as u8 {
            return (*header).region_id;
        }
        size *= 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::Pair;

    fn cons_obj() -> HeapObject {
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
    }

    #[test]
    fn alloc_obj_basic() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        let v = rp.alloc_obj(cons_obj(), &mut pool);
        assert!(v.is_heap());
        assert_eq!(rp.obj_count(), 1);
    }

    #[test]
    fn alloc_multiple_objects() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        let mut ptrs = vec![];
        for _ in 0..50 {
            let v = rp.alloc_obj(cons_obj(), &mut pool);
            ptrs.push(v.as_heap_ptr().unwrap() as usize);
        }
        assert_eq!(rp.obj_count(), 50);
        // All pointers should be unique.
        let unique: std::collections::HashSet<_> = ptrs.iter().collect();
        assert_eq!(unique.len(), 50);
    }

    #[test]
    fn alloc_inline_slice_basic() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        let s = rp.alloc_inline_slice(b"hello world", &mut pool);
        assert_eq!(s.as_slice(), b"hello world");
    }

    #[test]
    fn dual_ended_layout() {
        // Objects and data should coexist on the same page.
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);

        // Allocate some inline data first.
        let s = rp.alloc_inline_slice(b"test data", &mut pool);
        // Then an object.
        let v = rp.alloc_obj(cons_obj(), &mut pool);

        // Both should be on the same page (single 4K page).
        assert_eq!(rp.pages.len(), 1);
        assert_eq!(s.as_slice(), b"test data");
        assert!(v.is_heap());
    }

    #[test]
    fn page_growth_when_full() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);

        // Fill a 4K page: (4096 - 8) / 48 = ~85 objects max
        // Allocate enough to spill into a second page.
        for _ in 0..100 {
            rp.alloc_obj(cons_obj(), &mut pool);
        }
        assert!(rp.pages.len() >= 2, "should have claimed multiple pages");
        assert_eq!(rp.obj_count(), 100);
    }

    #[test]
    fn teardown_returns_pages() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        for _ in 0..10 {
            rp.alloc_obj(cons_obj(), &mut pool);
        }
        rp.teardown(&mut pool);
        assert_eq!(rp.obj_count(), 0);
        assert!(rp.pages.is_empty());
        // Pages should be in the pool's cache now.
        assert!(pool.cached_bytes() > 0);
    }

    #[test]
    fn teardown_runs_dtors() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);

        // Allocate an LBox (needs Drop for Rc).
        let cell = Rc::new(RefCell::new(Value::NIL));
        let weak = Rc::downgrade(&cell);
        let obj = HeapObject::LBox {
            cell,
            traits: Value::NIL,
        };
        let _v = rp.alloc_obj(obj, &mut pool);
        assert_eq!(rp.dtors.len(), 1);

        rp.teardown(&mut pool);
        // After teardown, the Rc should have been dropped.
        assert!(weak.upgrade().is_none(), "Rc should be dropped by teardown");
    }

    #[test]
    fn alloc_empty_inline_slice() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        let s = rp.alloc_inline_slice::<u8>(&[], &mut pool);
        assert_eq!(s.as_slice(), &[] as &[u8]);
    }

    #[test]
    fn owns_detects_region_pointers() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        let v = rp.alloc_obj(cons_obj(), &mut pool);
        let ptr = v.as_heap_ptr().unwrap();
        assert!(rp.owns(ptr));

        let x: i64 = 42;
        assert!(!rp.owns(&x as *const _ as *const ()));
    }

    #[test]
    fn page_header_region_id() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(42, 4096);
        let v = rp.alloc_obj(cons_obj(), &mut pool);

        // Read region_id from the page header.
        let ptr = v.as_heap_ptr().unwrap();
        let rid = unsafe { region_of_page_ptr(ptr, 4096) };
        assert_eq!(rid, 42);
    }

    #[test]
    fn large_inline_data_claims_bigger_page() {
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(1, 4096);
        // Allocate data larger than a 4K page.
        let big_data = vec![0xABu8; 8000];
        let s = rp.alloc_inline_slice(&big_data, &mut pool);
        assert_eq!(s.as_slice(), &big_data[..]);
    }

    #[test]
    fn region_of_page_ptr_on_grown_page() {
        // After geometric growth, objects may be on pages larger than 4K.
        // region_of_page_ptr must still find the correct region ID.
        let mut pool = PagePool::default();
        let mut rp = RegionPool::new(99, 4096);

        // Fill the first 4K page to force a second (8K) page.
        for _ in 0..100 {
            rp.alloc_obj(cons_obj(), &mut pool);
        }
        assert!(rp.pages.len() >= 2, "should have grown to multiple pages");

        // Allocate on the grown page.
        let v = rp.alloc_obj(cons_obj(), &mut pool);
        let ptr = v.as_heap_ptr().unwrap();
        let rid = unsafe { region_of_page_ptr(ptr, 4096) };
        assert_eq!(rid, 99, "region_of_page_ptr must work on grown pages");
    }
}
