// audited: 2026-10-21
//! The producer summary: the subtree a producer hands out, its root, and the interior owner edges that build it.
//!
//! docs/impl/region/owner.md

use super::*;

/// One producer summary: interior owner edges by emit site and kind, plus the
/// returned subtree's root.
pub(super) struct Summary {
    pub root: Region,
    /// `(emit site, member, owner)` — store/funnel-site edges.
    pub store_edges: Vec<(HirId, Region, Region)>,
    /// `(closure-construction site, member, owner)` — capture edges.
    pub capture_edges: Vec<(HirId, Region, Region)>,
}

/// The single region a producer's value resolves to, through the structural
/// wrappers, when the value is one binding read with exactly one source region.
/// A branch mix, a bare call, or an aggregate tail has no single region to hand
/// the consumer's value-resolved adopt.
pub(super) fn tail_root(h: &Hir, info: &RegionInfo) -> Option<Region> {
    match &h.kind {
        HirKind::Return { value } => tail_root(value, info),
        HirKind::Begin(es) => tail_root(es.last()?, info),
        HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => tail_root(body, info),
        HirKind::DerefCell { cell } => tail_root(cell, info),
        HirKind::Var(b) => match info.binding_source_regions.get(b)?.as_slice() {
            [r] => Some(*r),
            _ => None,
        },
        _ => None,
    }
}

impl Cut<'_> {
    /// The subtree `producer` returns through `tail`, or `None` when any gate
    /// refuses. The producer is a lambda on the call and fiber faces, whose tail is
    /// its body, and a binding's initializer on the inline face, which is its own
    /// tail.
    pub(super) fn summarize(&self, producer: &Hir, tail: &Hir) -> Option<Summary> {
        let info = self.info;
        let inputs = self.inputs;
        let inside = self.interval(producer);
        let inside_node = |id: HirId| inside.holds(self.ord(id));
        let root = tail_root(tail, info)?;
        // The root is born in the producer, is fresh (a Fresh call-result or a live
        // local allocation — an opaque result could be a borrow), crosses no fiber
        // frontier, and carries no dynamic-lifetime class.
        if !inside_node(self.alloc_site(root)?) {
            return None;
        }
        let root_fresh = info.fresh_result_regions.contains(&root)
            || (info.live_regions.contains(&root) && !info.call_result_regions.contains(&root));
        if !root_fresh
            || self.fiber_seeds.contains(&root)
            || info.cell_release_regions.contains(&root)
            || info.suppressed_decref_regions.contains(&root)
            || info.mutated_binding_value_regions.contains(&root)
            || self.is_merged(root)
        {
            return None;
        }
        // Members: born and last-used inside the producer, no frontier, no dynamic
        // class, sole-held, unclaimed.
        let subtree = inputs.reach(root);
        for &m in &subtree {
            if m == root {
                continue;
            }
            if inputs.not_ownable(info, m) || !inputs.sole_held(m) || self.is_merged(m) {
                return None;
            }
            let alloc = self.alloc_site(m)?;
            let dp = info.region_data.get(&m).map(|d| d.decref_point)?;
            if !inside_node(alloc) || !inside_node(dp) {
                return None;
            }
        }
        // External uniqueness: nothing inside references out (the hand-off itself
        // records no edge).
        if inputs.outside_ref_in(&subtree) {
            return None;
        }
        // The subtree must contain an interior cycle: an acyclic returned subtree
        // reclaims promptly by the RC cascade, and adopting it would only trade
        // that promptness for the activation bound.
        let has_cycle = subtree.iter().any(|&m| {
            inputs
                .reach(m)
                .iter()
                .any(|&m2| m2 != m && inputs.reach(m2).contains(&m))
        });
        if !has_cycle {
            return None;
        }
        self.owner_edges(root, &subtree, inside)
    }

    /// Give each non-root member of `subtree` its single owner, exactly as the
    /// store and capture adopts assign one: the root when a direct edge exists,
    /// else the unique interior container. The chosen edge must be emittable.
    fn owner_edges(
        &self,
        root: Region,
        subtree: &FxHashSet<Region>,
        inside: Interval,
    ) -> Option<Summary> {
        let info = self.info;
        let interior_store: Vec<(HirId, Region, Region)> = info
            .cross_region_refs
            .iter()
            .copied()
            .filter(|(site, s, d)| {
                !info.hard_edge_sites.contains(site) && subtree.contains(s) && subtree.contains(d)
            })
            .collect();
        let interior_capture: Vec<(HirId, Region, Region)> = self
            .inputs
            .capture_edges()
            .iter()
            .copied()
            .filter(|(_l, s, d)| subtree.contains(s) && subtree.contains(d))
            .collect();
        // The funnel store face: a containment edge is emittable at its own funnel
        // call inside the producer, when that call is a retaining store recording
        // the stored value `s`. A `%del` or a key read never retains. The adopt is
        // value-resolved there and needs no store opcode.
        let interior_funnel: Vec<(HirId, Region, Region)> = info
            .containment_edges
            .iter()
            .copied()
            .filter(|&(site, s, d)| {
                subtree.contains(&s)
                    && subtree.contains(&d)
                    && inside.holds(self.ord(site))
                    && info
                        .funnel_store_sites
                        .get(&site)
                        .is_some_and(|stored| stored.contains(&s))
            })
            .collect();
        let mut containers_of: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
        for &(_, s, d) in interior_store
            .iter()
            .chain(interior_capture.iter())
            .chain(interior_funnel.iter())
        {
            containers_of.entry(s).or_default().insert(d);
        }
        let mut store_edges: Vec<(HirId, Region, Region)> = Vec::new();
        let mut cap_edges: Vec<(HirId, Region, Region)> = Vec::new();
        for &m in subtree {
            if m == root {
                continue;
            }
            let owner = match containers_of.get(&m) {
                Some(cs) if cs.contains(&root) => root,
                Some(cs) if cs.len() == 1 => *cs.iter().next()?,
                _ => return None,
            };
            // Prefer a store/funnel site (the edge's own site), else the capture's
            // closure-construction site.
            if let Some(&(site, s, d)) = interior_store
                .iter()
                .chain(interior_funnel.iter())
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                store_edges.push((site, s, d));
            } else if let Some(&(lambda, s, d)) = interior_capture
                .iter()
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                cap_edges.push((lambda, s, d));
            } else {
                return None;
            }
        }
        Some(Summary {
            root,
            store_edges,
            capture_edges: cap_edges,
        })
    }
}
