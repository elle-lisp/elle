// audited: 2026-09-15
//! The queries over [`RegionInfo`]: what a binding holds, what an operand
//! hands a call, and where the merge forest sends a region.
//!
//! docs/impl/region/merging.md
//! docs/impl/region/relocate.md

use super::super::Region;
use super::RegionInfo;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use rustc_hash::FxHashSet;

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

    /// Every region one tail-call OPERAND — the callee, or an argument — may
    /// hand the call: the regions the value it PRODUCES names, never the
    /// regions its evaluation merely used along the way.
    ///
    /// Each node in a value-producing position contributes its own
    /// `alloc_region` and, for a `Var`, its binding's source regions,
    /// canonicalized through the merge forest. The descent stops at a `Call`
    /// and at a `Lambda`, whose values each carry a count of their own, and
    /// passes through everything else — an inline `%`-`Intrinsic` included,
    /// which mints no region. Descending an unrecognised node
    /// over-approximates, the leak-preserving direction.
    ///
    /// Read by the lowerer's frame-exit exemption and by the branch-arm
    /// window's per-point funding question.
    ///
    /// docs/impl/region/relocate.md
    pub fn operand_value_regions(&self, h: &Hir, out: &mut FxHashSet<Region>) {
        if let Some(&r) = self.alloc_region.get(&h.id) {
            out.insert(self.merged_root(r));
        }
        if let HirKind::Var(b) = &h.kind {
            for &r in self.binding_source_regions.get(b).into_iter().flatten() {
                out.insert(self.merged_root(r));
            }
        }
        if matches!(h.kind, HirKind::Call { .. } | HirKind::Lambda { .. }) {
            return;
        }
        h.for_each_child(|c| self.operand_value_regions(c, out));
    }

    /// The placeholder minted for the collection `binding` reaches at `node`,
    /// or `None` where the node's pattern built none for it. One lookup answers
    /// for either kind of name, the rest name and a name a further pattern
    /// bound alike, because both must outlive the collection.
    ///
    /// docs/impl/region/anchors.md
    pub fn rest_collection_region(&self, node: HirId, binding: Binding) -> Option<Region> {
        self.pattern_rest_regions
            .get(&node)?
            .iter()
            .find(|c| c.holders.contains(&binding))
            .map(|c| c.region)
    }

    /// Does `binding` HOLD `region` as the collection its rest pattern built,
    /// rather than merely name it? A rest name does both, so the question is
    /// asked per region. A name a further pattern bound projects the collection
    /// and holds nothing, so it answers `false` and keeps the slot comparison.
    /// `region` is a merged root.
    ///
    /// docs/impl/region/relocate.md
    pub fn holds_built_rest_collection(&self, binding: Binding, region: Region) -> bool {
        self.pattern_rest_regions
            .values()
            .flatten()
            .any(|c| c.bound_name == Some(binding) && self.merged_root(c.region) == region)
    }

    /// Does this scope have any allocations whose solved region matches it?
    pub fn scope_has_local_allocs(&self, hir_id: HirId) -> bool {
        self.scope_region
            .get(&hir_id)
            .is_some_and(|r| self.live_regions.contains(r))
    }

    /// The region a builder-idiom merge collapses `r` onto — the outermost
    /// ancestor in the `merged_parent` forest, or `r` itself when it is not a
    /// merge child. Bounded by the forest depth; the guard rejects a cycle,
    /// of which there are none by construction.
    ///
    /// docs/impl/region/merging.md
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
}
