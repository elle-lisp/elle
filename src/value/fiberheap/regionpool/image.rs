// audited: 2026-10-21
//! A region pool's image side: the pages a hydrated image maps, adopted with their cursors, and the layouts a dump copies.
//!
//! docs/impl/image.md

use std::ops::Range;

use super::{
    holds_value_refs, needs_drop, ImageSpan, MmapPage, RegionPage, RegionPool, HEADER_SIZE,
};
use crate::value::heap::HeapObject;

/// One page's byte layout, as [`RegionPool::page_layouts`] reports it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PageLayout {
    pub base: usize,
    pub len: usize,
    pub obj_cursor: usize,
    pub data_cursor: usize,
}

impl RegionPool {
    /// The address interval a hydrated image's pages tile, or `None` for a
    /// region that hydrated nothing.
    #[cfg(test)]
    pub(crate) fn image_interval(&self) -> Option<Range<usize>> {
        self.image.as_ref().map(|s| s.interval.clone())
    }

    /// Whether `addr` lies in the interval a hydrated image's pages tile.
    #[inline]
    pub(crate) fn image_contains(&self, addr: usize) -> bool {
        self.image
            .as_ref()
            .is_some_and(|s| s.interval.contains(&addr))
    }

    /// Byte layout of every page this region owns, in claim order. The image
    /// dumper reads these to copy page bytes and bound the meaningful spans
    /// (docs/impl/image.md).
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

    /// Adopt a hydrated image's pages (docs/impl/image.md), each with the
    /// cursors the image's page table recorded. Each header takes this region's
    /// identity, so later allocation into this region and the release spans
    /// both see the true layout. The pages tile one interval, which the pool
    /// records as its image.
    pub(crate) fn adopt_image(&mut self, pages: Vec<(MmapPage, usize, usize)>) {
        debug_assert!(
            self.pages.is_empty() && self.image.is_none(),
            "an image is adopted into a fresh pool"
        );
        let mut interval: Option<Range<usize>> = None;
        for (page, obj_cursor, data_cursor) in pages {
            debug_assert!(
                HEADER_SIZE <= obj_cursor && obj_cursor <= data_cursor && data_cursor <= page.len(),
                "hydrated page cursors out of order: {obj_cursor}..{data_cursor} in {}",
                page.len(),
            );
            let base = page.as_ptr() as usize;
            let end = base + page.len();
            interval = Some(match interval {
                None => base..end,
                Some(i) => i.start.min(base)..i.end.max(end),
            });
            let mut rp = RegionPage::new(page, self.region_id, self.stamp);
            rp.obj_cursor = obj_cursor;
            rp.data_cursor = data_cursor;
            self.pages.push(rp);
        }
        if let Some(interval) = interval {
            debug_assert_eq!(
                interval.len(),
                self.allocated_bytes(),
                "hydrated pages leave a gap in their interval"
            );
            self.image = Some(ImageSpan {
                interval,
                pages: self.pages.len(),
            });
        }
    }

    /// Rebuild the object bookkeeping from an image's object index
    /// (docs/impl/image.md): route each object to `dtors` or `ref_objs` by the
    /// same predicates the alloc path uses, and count it.
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
}
