// audited: 2026-09-14
//! The cross-region store edges the walk records at a site, and the may-store
//! clique a declared-store native contributes.
//!
//! docs/impl/region/effects.md

use super::*;

impl RegionInference {
    /// Record a cross-region edge `src → dst` at the storage site
    /// `hir_id`. Skips self-edges (src == dst).
    pub(super) fn record_edge(&mut self, hir_id: HirId, src: Region, dst: Region) {
        if src != dst {
            self.cross_region_refs.push((hir_id, src, dst));
        }
    }

    /// Record the may-store edges for a declared-store native call at `site`: from
    /// each listed (stored) argument's regions to every OTHER heap argument's
    /// regions (the possible in-argument store targets), and mark the site HARD (the
    /// lowerer increfs a call-result source by value; docs/impl/region/effects.md
    /// "Hard edges"). The `Stores` effect alone reaches this: a `Sends` store is
    /// seam-counted at runtime and records no edge (the `Sends` arm in
    /// `walk_call`).
    pub(super) fn record_store_edges(
        &mut self,
        site: HirId,
        stored: &[usize],
        arg_regions: &[Vec<Region>],
    ) {
        self.hard_edge_sites.insert(site);
        for &i in stored {
            let Some(src_rs) = arg_regions.get(i) else {
                continue;
            };
            let src_rs = src_rs.clone();
            for (j, dst_rs) in arg_regions.iter().enumerate() {
                if j == i {
                    continue;
                }
                let dst_rs = dst_rs.clone();
                for &src in &src_rs {
                    for &dst in &dst_rs {
                        self.record_edge(site, src, dst);
                    }
                }
            }
        }
    }
}
