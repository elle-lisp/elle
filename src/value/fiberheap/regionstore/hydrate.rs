//! Installing a hydrated image region (docs/impl/image.md § Hydration
//! steps 3–6): the mapped, relocated pages become the pages of a freshly
//! minted `Counted` region, indistinguishable from any other region to RC,
//! teardown, and the diagnostics. The mapping and relocation themselves
//! happen in `crate::image::hydrate`; this is the region-store seam.

use super::super::pagepool::MmapPage;
use super::*;
use crate::value::heap::HeapTag;

impl RegionStore {
    /// Mint a region and adopt `pages` (each with the cursors the image's
    /// page table recorded) plus the object index (`base`-relative offsets).
    /// The region is `Counted(1)` — the caller holds the one reference.
    pub(crate) fn install_hydrated_region(
        &mut self,
        pages: Vec<(MmapPage, usize, usize)>,
        objects: &[(usize, HeapTag)],
        base: usize,
    ) -> RuntimeRegion {
        let id = self.new_runtime_region();
        self.ensure(id);
        let entry = self.regions[id.get() as usize]
            .as_mut()
            .expect("ensure created the entry");
        for (page, obj_cursor, data_cursor) in pages {
            entry
                .pool
                .adopt_hydrated_page(page, obj_cursor, data_cursor);
        }
        let ptrs: Vec<(*mut crate::value::heap::HeapObject, HeapTag)> = objects
            .iter()
            .map(|&(off, tag)| ((base + off) as *mut crate::value::heap::HeapObject, tag))
            .collect();
        entry.pool.install_object_index(&ptrs);
        id
    }

    /// The pool of one live region (read-only), for the image dumper's page
    /// and object walks. `None` for an absent region.
    pub(crate) fn region_pool(&self, id: RuntimeRegion) -> Option<&RegionPool> {
        self.regions
            .get(id.get() as usize)
            .and_then(|s| s.as_ref())
            .map(|e| &e.pool)
    }
}
