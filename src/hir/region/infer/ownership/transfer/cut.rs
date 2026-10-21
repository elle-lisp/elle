// audited: 2026-10-20
//! The facts every face of the transfer cut reads, gathered once per unit.
//!
//! docs/impl/region/owner.md
//!
//! The facts are the ownership inputs, the use index, the post-order intervals,
//! each region's allocation site, and the regions the fiber frontier seeds.

use super::*;

/// A node's post-order subtree interval: `low` is the first descendant's order,
/// `top` the node's own. A node is inside when its order falls in the interval.
#[derive(Clone, Copy)]
pub(super) struct Interval {
    low: u32,
    top: u32,
}

impl Interval {
    /// Whether order `o` is the node itself or one of its descendants.
    pub(super) fn holds(self, o: u32) -> bool {
        self.low <= o && o <= self.top
    }

    /// Whether order `o` is a proper descendant: inside, and not the node.
    pub(super) fn holds_below(self, o: u32) -> bool {
        self.low <= o && o < self.top
    }
}

/// The per-unit context the faces share.
pub(super) struct Cut<'a> {
    pub inputs: &'a OwnershipInputs,
    pub info: &'a RegionInfo,
    pub ix: UseIndex,
    /// Bindings some lambda captures. A captured fiber binding refuses the fiber face.
    pub captured: FxHashSet<Binding>,
    order: &'a Order,
    /// Region → allocation site: real allocations and prebound capture cells, as in
    /// the group and activation walks. The structural key for the born-inside and
    /// dies-inside gates.
    region_alloc_hir: FxHashMap<Region, HirId>,
    /// The fiber-frontier seed halves (emit / send). A returned root may cross the
    /// RETURN frontier — that is the shape — but an emitted or sent root has an
    /// unbounded second consumer and refuses.
    pub fiber_seeds: FxHashSet<Region>,
}

impl<'a> Cut<'a> {
    pub(super) fn new(
        inputs: &'a OwnershipInputs,
        hir: &Hir,
        info: &'a RegionInfo,
        escape: &crate::hir::EscapeInfo,
        arena: &BindingArena,
        call_class: &CallClassification,
        order: &'a Order,
    ) -> Self {
        let mut region_alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
        for (&hid, &reg) in &info.alloc_region {
            region_alloc_hir.insert(reg, hid);
        }
        for (&begin_id, cells) in &info.begin_cell_regions {
            for &(_b, reg) in cells {
                region_alloc_hir.insert(reg, begin_id);
            }
        }
        let mut fiber_seeds: FxHashSet<Region> = FxHashSet::default();
        for (&b, regions) in &info.binding_source_regions {
            if escape.escapes_fiber(b) {
                fiber_seeds.extend(regions.iter().copied());
            }
        }
        for (&hid, &r) in &info.alloc_region {
            if escape.escapes_fiber_frontier(hid) {
                fiber_seeds.insert(r);
            }
        }
        Cut {
            inputs,
            info,
            ix: UseIndex::build(hir, arena, call_class),
            captured: captured_bindings(hir),
            order,
            region_alloc_hir,
            fiber_seeds,
        }
    }

    /// The post-order position of a node.
    pub(super) fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// The post-order subtree interval of `node`.
    pub(super) fn interval(&self, node: &Hir) -> Interval {
        Interval {
            low: self.order.low(&node.id).unwrap_or(0),
            top: self.ord(node.id),
        }
    }

    /// The node that allocates `r`, if any.
    pub(super) fn alloc_site(&self, r: Region) -> Option<HirId> {
        self.region_alloc_hir.get(&r).copied()
    }

    /// Whether the merge forest (builder-idiom or closure-cycle) already claims `r`:
    /// never a transfer member (the one-owner invariant).
    pub(super) fn is_merged(&self, r: Region) -> bool {
        self.info.merged_parent.contains_key(&r)
            || self.info.merged_parent.values().any(|&p| p == r)
            || self.info.closure_cycle_members.contains(&r)
    }

    /// Whether `r` is touched by any edge of any kind, in either role. Hard
    /// may-stores count: a may-holder may hold. Embed edges count: a fresh
    /// result holds what it embeds.
    pub(super) fn in_any_edge(&self, r: Region) -> bool {
        let info = self.info;
        info.cross_region_refs
            .iter()
            .any(|&(_, s, d)| s == r || d == r)
            || self
                .inputs
                .capture_edges()
                .iter()
                .any(|&(_, s, d)| s == r || d == r)
            || info
                .containment_edges
                .iter()
                .chain(&info.embed_edges)
                .any(|&(_, s, d)| s == r || d == r)
    }

    /// Whether every recorded read of `b` is an argument of an `Immediate`
    /// native. Extraction through a pass-through native records no edge, so this
    /// shape gate is what keeps an uncounted member borrow from escaping the
    /// node's reclamation horizon.
    pub(super) fn reads_pass(&self, b: Binding) -> bool {
        self.ix
            .uses
            .get(&b)
            .is_none_or(|uses| uses.iter().all(|u| matches!(u, UseForm::ImmediateArg)))
    }

    /// Whether every recorded init of `b` is a proper descendant of the node
    /// whose interval is `inside`: a binding the producer introduces, whose reads
    /// precede the hand-off.
    pub(super) fn bound_inside(&self, b: Binding, inside: Interval) -> bool {
        self.ix.inits.get(&b).is_some_and(|inits| {
            inits.iter().all(|&i| {
                // SAFETY: init pointers index the HIR tree, which outlives this call.
                inside.holds_below(self.ord(unsafe { &*i }.id))
            })
        })
    }
}
