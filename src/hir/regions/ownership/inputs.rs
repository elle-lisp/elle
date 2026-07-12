use super::super::*;
use super::capture::capture_containment_edges;
use super::seeds::compute_shared_seeds;
use rustc_hash::{FxHashMap, FxHashSet};

/// The shared containment-graph + ownership-candidate inputs read by BOTH the
/// externally-unique subtree walk ([`compute_owned_subtrees`]) and the co-owned-cycle
/// walk ([`compute_owned_region_groups`]): the eligible-edge child map, the `contained`
/// set, the Shared-seed set, the re-derived capture edges, the holder index, and the
/// real-allocation candidate set. Factored so the two walks read the *same* graph by
/// construction — a divergence between them would be an ownership-soundness bug.
pub(super) struct OwnershipInputs {
    /// `parent → children` over ELIGIBLE containment edges (non-hard `cross_region_refs`
    /// stores + capture + funnel-recovered containment): a parent contains each child
    /// (`target ⊇ source`). A `hard_edge_site` may-store does not build the subtree (it
    /// still counts against external uniqueness — see [`Self::outside_ref_in`]).
    children_of: FxHashMap<Region, Vec<Region>>,
    /// Every region that is some parent's child — i.e. not a top container.
    pub(super) contained: FxHashSet<Region>,
    /// The Shared-seed set (`compute_shared_seeds`): regions that cross a frontier.
    shared: FxHashSet<Region>,
    /// Region → distinct user holders, for the sole-held check.
    region_holders: super::super::holders::RegionHolders,
    /// Real allocations (`alloc_region` sites + pre-allocated capture cells) — the
    /// candidate region set both walks iterate.
    pub(super) alloc_regions: FxHashSet<Region>,
    /// `source → targets` over ALL edges the external-uniqueness scan reads (every
    /// `cross_region_refs` store — hard edges INCLUDED — plus the capture and funnel
    /// containment edges). Indexing by source once lets [`Self::outside_ref_in`] examine
    /// only the edges leaving a subtree's own members, instead of re-scanning every edge in
    /// the compilation unit per candidate subtree (which was O(subtrees × edges), quadratic
    /// on the whole-stdlib letrec).
    out_edges_by_src: FxHashMap<Region, Vec<Region>>,
}

/// Build the [`OwnershipInputs`] for a compilation unit (the containment graph and the
/// candidate set), shared by the two ownership walks. Mirrors `build_info`'s
/// `cross_region_refs` filtering: only live source regions yield capture edges (done in
/// [`capture_containment_edges`]).
pub(super) fn ownership_inputs(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
) -> OwnershipInputs {
    let capture_edges = capture_containment_edges(hir, info, arena);
    let shared = compute_shared_seeds(info, escape);
    // Region → distinct user holders (any non-synthetic user binding is a holder), the
    // shared sole-held index (`regions::holders`).
    let region_holders = super::super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );
    // The eligible containment edges (`target ⊇ source`): non-hard stores, capture, and
    // funnel-recovered containment. `children_of[parent]` lists a parent's children;
    // `contained` is every child (a non-top-container).
    let mut children_of: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    let mut contained: FxHashSet<Region> = FxHashSet::default();
    for &(site, src, dst) in &info.cross_region_refs {
        if info.hard_edge_sites.contains(&site) {
            continue;
        }
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    for &(_lambda, src, dst) in &capture_edges {
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    for &(_site, src, dst) in &info.containment_edges {
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    let alloc_regions: FxHashSet<Region> = info
        .alloc_region
        .values()
        .copied()
        .chain(
            info.begin_cell_regions
                .values()
                .flat_map(|v| v.iter().map(|(_, r)| *r)),
        )
        .collect();
    // Source-indexed edges for the external-uniqueness scan. Unlike `children_of` (subtree
    // BUILD, hard edges excluded), this includes HARD `cross_region_refs` stores — a
    // may-store from outside still breaks external uniqueness (see `outside_ref_in`).
    let mut out_edges_by_src: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    for &(_site, src, dst) in &info.cross_region_refs {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    for &(_lambda, src, dst) in &capture_edges {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    for &(_site, src, dst) in &info.containment_edges {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    OwnershipInputs {
        children_of,
        contained,
        shared,
        region_holders,
        alloc_regions,
        out_edges_by_src,
    }
}

impl OwnershipInputs {
    /// A region is never statically ownable when it crosses a frontier (a Shared seed) or
    /// has a runtime-determined identity/lifetime: a non-`Fresh` `call_result` placeholder
    /// (a possible borrow / opaque result — but a `Fresh` call-result is a genuinely
    /// caller-owned allocation and IS ownable), a `cell_release` placeholder, a reassigned
    /// `mutated_binding_value` region, or a `suppressed_decref` region.
    pub(super) fn not_ownable(&self, info: &RegionInfo, r: Region) -> bool {
        self.shared.contains(&r)
            || (info.call_result_regions.contains(&r) && !info.fresh_result_regions.contains(&r))
            || info.cell_release_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || info.suppressed_decref_regions.contains(&r)
    }

    /// Sole-held: at most one distinct user binding holds `r` (an ANF-temp-only region
    /// has no user holder and is sole-held by construction).
    pub(super) fn sole_held(&self, r: Region) -> bool {
        self.region_holders
            .holders_of(r)
            .is_none_or(|hs| hs.len() <= 1)
    }

    /// The single user binding holding `r`, when exactly one does (`None` for a
    /// holderless or aliased region). The activation-adopt cut requires each SCC
    /// member to have its OWN holder binding — the slot its value-resolved
    /// `AdoptIntoActivation` loads — pairwise distinct across the members.
    pub(super) fn sole_holder(&self, r: Region) -> Option<Binding> {
        self.region_holders.holders_of(r).and_then(|hs| {
            if hs.len() == 1 {
                hs.iter().next().copied()
            } else {
                None
            }
        })
    }

    /// `r` plus every region transitively contained in it, following the eligible edges
    /// from a parent in the set to its children. A set closure, so an interior reference
    /// cycle terminates.
    pub(super) fn reach(&self, r: Region) -> FxHashSet<Region> {
        let mut set: FxHashSet<Region> = FxHashSet::default();
        set.insert(r);
        let mut work = vec![r];
        while let Some(parent) = work.pop() {
            if let Some(kids) = self.children_of.get(&parent) {
                for &c in kids {
                    if set.insert(c) {
                        work.push(c);
                    }
                }
            }
        }
        set
    }

    /// External-uniqueness failure: some edge — over ALL `cross_region_refs` (hard edges
    /// included), the capture edges, and the funnel containment — has its source INSIDE
    /// `set` and its target OUTSIDE it (an outside container holding an interior region,
    /// which freeing `set` as a unit would dangle). Reads the source-indexed `out_edges_by_src`
    /// so only edges LEAVING `set`'s own members are examined (not every edge in the unit).
    pub(super) fn outside_ref_in(&self, set: &FxHashSet<Region>) -> bool {
        set.iter().any(|m| {
            self.out_edges_by_src
                .get(m)
                .is_some_and(|dsts| dsts.iter().any(|d| !set.contains(d)))
        })
    }
}
