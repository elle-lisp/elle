// audited: 2026-11-12
//! The queries over [`RegionInfo`]: what a binding holds, which cell or collection a node minted, and where a merge sends a region.
//!
//! docs/impl/region/merging.md
//! docs/impl/region/restlist.md

use super::super::Region;
use super::RegionInfo;
use crate::hir::binding::Binding;
use crate::hir::expr::HirId;
use crate::value::RestListLayout;

impl RegionInfo {
    /// The compiled capture-cell region for `binding`, ONLY when the binding
    /// minted exactly ONE across all `begin_cell_regions` scopes. `None` for a
    /// binding with no compiled cell, and `None` for one with more than a
    /// single distinct cell. The ownership forest's `closure ⊇ cell` re-point
    /// and its `AdoptCellRegion` emit both gate on this, so the analysis and the
    /// lowerer name the same cell or agree to refuse.
    ///
    /// docs/impl/region/cells.md
    pub fn single_cell_region_of(&self, binding: Binding) -> Option<Region> {
        let mut found: Option<Region> = None;
        for cells in self.begin_cell_regions.values() {
            for &(b, r) in cells {
                if b == binding {
                    match found {
                        None => found = Some(r),
                        Some(prev) if prev == r => {}
                        // A second, distinct cell for the same binding — ambiguous.
                        Some(_) => return None,
                    }
                }
            }
        }
        found
    }

    /// Whether `region` is the region a compiled capture cell was minted in,
    /// which names the box and never the content a read through it hands back.
    ///
    /// docs/impl/region/mechanism.md
    pub fn is_compiled_cell_region(&self, region: Region) -> bool {
        self.begin_cell_regions
            .values()
            .flatten()
            .any(|&(_, r)| r == region)
    }

    /// The placeholder minted for the `index`-th collection `node`'s pattern
    /// builds, counting in the order `lower_destructure` reaches them
    /// (`HirPattern::building_rests`).
    ///
    /// The lookup a build site makes, and the only one that answers for a
    /// collection no name of the program reaches.
    ///
    /// docs/impl/region/anchors.md
    pub fn rest_collection_at(&self, node: HirId, index: usize) -> Option<Region> {
        self.pattern_rest_regions
            .get(&node)?
            .get(index)
            .map(|c| c.region)
    }

    /// The placeholder minted for the collection `binding` reaches at `node`,
    /// or `None` where the node's pattern built none for it. One lookup answers
    /// for either kind of name, the rest name and a name a further pattern
    /// bound alike, because both must outlive the collection.
    ///
    /// The lookup a decision tree makes, which reaches a rest name through its
    /// access path and has no build order to count with.
    ///
    /// docs/impl/region/anchors.md
    pub fn rest_collection_region(&self, node: HirId, binding: Binding) -> Option<Region> {
        self.pattern_rest_regions
            .get(&node)?
            .iter()
            .find(|c| c.holders.contains(&binding))
            .map(|c| c.region)
    }

    /// Does `binding` reach `region` as a collection a rest pattern BUILT,
    /// rather than as a region it merely names? Such a collection is routed
    /// through a slot of the lowerer's own that no call passes, so the slot
    /// comparison can only ever read it as unmoved. Every other region the name
    /// carries is the scrutinee's, so the question is asked per region.
    /// `region` is a merged root.
    pub fn names_built_rest_collection(&self, binding: Binding, region: Region) -> bool {
        self.pattern_rest_regions
            .values()
            .flatten()
            .any(|c| c.holders.contains(&binding) && self.merged_root(c.region) == region)
    }

    /// Does any allocation lie under this scope's region? True when the
    /// scope's region is in `live_regions`.
    pub fn scope_has_local_allocs(&self, hir_id: HirId) -> bool {
        self.scope_region
            .get(&hir_id)
            .is_some_and(|r| self.live_regions.contains(r))
    }

    /// The region a builder-idiom merge collapses `r` onto — the outermost
    /// ancestor in the `merged_parent` forest, or `r` itself when it is not a
    /// merge child. Bounded by the forest depth. A guard stops the walk after
    /// 10,000 steps, which only a cycle could reach, and the forest has none by
    /// construction.
    pub fn merged_root(&self, r: Region) -> Region {
        let mut cur = r;
        let mut guard = 0u32;
        while let Some(&parent) = self.merged_parent.get(&cur) {
            cur = parent;
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }
        cur
    }

    /// True when the cross-region store edge `source → target` becomes an
    /// intra-region SELF-EDGE once the builder-idiom merge collapses both
    /// endpoints onto one physical region. `emit_increfs_for` drops such an
    /// edge's `IncrefRegion`, which the free-time cascade would never balance.
    /// `record_edge` already drops a raw `source == target` edge, so this fires
    /// only on a merge-collapsed one.
    ///
    /// docs/impl/region/mechanism.md
    pub fn is_merge_self_edge(&self, source: Region, target: Region) -> bool {
        self.merged_root(source) == self.merged_root(target)
    }

    /// True when the store edge at `site` targets the object allocated at that
    /// very site: a `%pair`'s car or cdr. The allocation's scan counts the
    /// reference itself (`alloc_obj`), so the edge takes no `IncrefRegion`. The
    /// merge seed reads the same test as the builder idiom's store.
    ///
    /// docs/impl/region/mechanism.md
    pub fn alloc_counts_edge(&self, site: HirId, target: Region) -> bool {
        self.alloc_region.get(&site) == Some(&target)
    }

    /// How the `&` rest list of the lambda at `lambda` is built: in one region
    /// when the gate admitted it, one region per cell otherwise.
    ///
    /// docs/impl/region/restlist.md
    pub fn rest_list_layout(&self, lambda: HirId) -> RestListLayout {
        if self.one_region_rest_lists.contains(&lambda) {
            RestListLayout::OneRegion
        } else {
            RestListLayout::PerCell
        }
    }
}
