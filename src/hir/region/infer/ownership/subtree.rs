// audited: 2026-10-21
//! The ownership cuts that free a whole subtree under one root, and a rootless cycle as one group.
//!
//! docs/impl/region/ownership.md
//! docs/impl/region/adopt.md

use super::super::*;
use super::capture::closure_regions;
use super::inputs::OwnershipInputs;
use rustc_hash::{FxHashMap, FxHashSet};

mod scope;
pub(in crate::hir::region::infer) use scope::innermost_enclosing_scope;

/// The externally-unique Owned subtrees of a compilation unit, the consumer of
/// `compute_shared_seeds`. Maps each Owned **root** region to the
/// full set of regions in its subtree (the root included). A region absent from every
/// value here is Shared (the conservative baseline: per-region RC, always legal).
///
/// ## What a subtree is: the containment graph, read outward
///
/// The edges are `RegionInfo::cross_region_refs`. Each `(site, source, target)`
/// means a value in `source` is stored into a structure in `target`. So at runtime
/// `target` holds a reference to `source`, and the region store's free cascade
/// frees `source` when `target` dies. In ownership terms **`target` is the parent
/// and `source` the child**. The subtree rooted at a container `r` is `r` plus
/// every region transitively *contained in* it. The walk follows edges from a
/// parent already in the set to its children (`target ∈ S ⇒ add source`).
///
/// Only **eligible** edges build the subtree. An edge recorded at one of the
/// `hard_edge_sites` is a native may-store/clique over-approximation
/// (`Mixed`/`Unknown`/declared `Stores`). Ownership cannot be proven *through* a
/// may-store, so such edges do not extend `Reach`, though they still count
/// *against* external uniqueness.
///
/// The graph also carries **capture edges** (`capture_containment_edges`). A
/// closure's region contains every value it captures, but a capture records *no*
/// `cross_region_refs` edge, because the runtime counts a closure's env itself. So
/// the edges are re-derived from the HIR. They are eligible, because a capture is
/// a genuine containment and not a may-store. Like every edge, they count against
/// external uniqueness.
///
/// The graph carries the **funnel-store and cell containment** too
/// (`RegionInfo::containment_edges`). One is a value a `Funnel` op stores into a
/// mutable retaining container (`@array`/`@struct`), recovered from the
/// container's `RetType`. On that native-call path the funnel records no
/// `cross_region_refs` edge. The other is a compiled capture cell's content. The
/// edges are eligible, and count against external uniqueness exactly like the
/// capture edges.
///
/// The **embed edges** (`RegionInfo::embed_edges`, a `Fresh` native's declared
/// `result ⊇ arg`) are read one way only: they count against external uniqueness
/// and build no subtree. There is no site at which the result could adopt the
/// argument. So a subtree built through the edge would refuse at the single-owner
/// filter, and take every container holding the result down with it
/// (docs/impl/region/effects.md).
///
/// ## When a subtree is Owned: external uniqueness
///
/// `S = {r} ∪ Reach(r)` is externally unique, and therefore Owned, iff:
///
/// 1. **No region in `S` crosses a frontier or has runtime-determined lifetime.**
///    No region in `S` is a Shared seed (`compute_shared_seeds`: return / emit /
///    send). None is one of the dynamic classes that are never statically
///    ownable: `call_result_regions` (a callee's result region, possibly a
///    borrow), `cell_release_regions`, `mutated_binding_value_regions`, and
///    `suppressed_decref_regions` (the runtime-counted reassign-cell values). With
///    any such member, the subtree drop could free a value that outlives `S` or
///    whose identity the compiler cannot bound, so the walk refuses.
/// 2. **Nothing outside `S` references into `S`.** No edge `(_, source, target)`
///    has `source ∈ S` and `target ∉ S`, over every edge kind: *all*
///    `cross_region_refs` with hard edges included, the capture edges, the
///    containment edges and the embed edges. Such an edge is an outside container
///    holding a reference to an interior region, and dropping `S` would dangle it.
///    The root's own single reference is the activation or binding that
///    sole-holds it. That is not a `cross_region_refs` edge, so construction
///    allows it. This check catches region-level aliasing (a child stored into two
///    parents) and a
///    may-store from outside.
///
/// A **candidate root** is a live region that is sole-held (≤1 distinct user
/// binding, through `RegionHolders`). It is also a *top container*: not the
/// `source` of any eligible edge, that is, not itself contained in another
/// region. Restricting roots to top containers yields maximal, non-overlapping
/// subtrees. The walk reaches an interior child from its parent, and a child
/// stored into two parents fails check 2 for both. So the accepted roots
/// partition disjoint subtrees.
///
/// The tight single-edge case (a fresh child stored once into one fresh parent, both
/// dying together) is exactly the builder-idiom MERGE
/// (`region::infer::merge`); this generalizes it to multi-edge components.
pub(in crate::hir::region::infer) fn compute_owned_subtrees(
    inputs: &OwnershipInputs,
    info: &RegionInfo,
) -> FxHashMap<Region, FxHashSet<Region>> {
    let mut owned: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
    for &r in &inputs.alloc_regions {
        // Candidate root: a sole-held, non-frontier TOP container, the child of no
        // eligible edge. A rootless cycle, whose members are all `contained`, is the
        // co-owned-cycle walk's job (`compute_owned_region_groups`).
        if inputs.not_ownable(info, r) || !inputs.sole_held(r) || inputs.contained.contains(&r) {
            continue;
        }
        let subtree = inputs.reach(r);
        // External uniqueness: (1) no interior region crosses a frontier or is a dynamic
        // class, and (2) nothing outside the subtree references in.
        if subtree.iter().any(|&m| inputs.not_ownable(info, m)) {
            continue;
        }
        if inputs.outside_ref_in(&subtree) {
            continue;
        }
        owned.insert(r, subtree);
    }
    owned
}

/// The co-owned region **groups**, the co-owned-cycle cut. A mutual reference
/// cycle with no container parent has no owner among its members: each owns and
/// is owned by the others. That is an externally-unique *source* strongly
/// connected component of the containment graph, for example
/// `(let [a (@array) b (@array)] (push a b) (push b a))`. So it is reclaimed
/// **symmetrically as one unit**, rather than by promoting a member to root, an
/// arbitrary choice with no lifetime meaning.
///
/// Maps each group's **drop site** to its member regions. The drop site is the
/// innermost structural scope ([`innermost_enclosing_scope`]) lexically enclosing
/// every member's allocation. There the lowerer emits one `FreeRegionGroup` over
/// the whole set, in place of the members' individual decrefs.
///
/// The drop site is the enclosing *scope*, **not** a comparison of the members'
/// `decref_point`s, because every **pass-through alias** of a member also derefs
/// its region. `%array-push`/`%put` return their container, so a discarded store
/// result's `result_region_of` derefs a member's region one structural step *past*
/// the member's own `decref_point`. Freeing the set at `max(member decref_point)`
/// frees those regions before the alias deref, a stale-deref UAF. The enclosing
/// scope node post-dominates the whole body, and so every in-scope pass-through
/// alias release. Structural nesting is the lifetime ordering; numeric order is
/// not.
///
/// A group qualifies iff all of these hold:
///
/// - its members form an SCC of size ≥ 2, a genuine cycle;
/// - every member is ownable and sole-held;
/// - the SCC owns no downstream members (`reach(SCC) == SCC`, the smallest-cut
///   restriction); a cycle that also owns a sub-tree stays on the always-legal
///   per-region-RC baseline;
/// - it is externally unique (`outside_ref_in` false), that is, a SOURCE in the
///   condensation that no external container holds a member of;
/// - every member and within-SCC store result is released inside the drop site.
///   The scope's value can carry a member out to a consumer past the scope, and
///   the scope's end does not post-dominate that consumer's read.
///
/// Disjoint from [`compute_owned_subtrees`] by construction. No top container
/// reaches a source SCC, because being reached would mean an external container
/// holds a member. So the container-rooted subtrees and the co-owned groups never
/// overlap. The ownership pass in `analyze_regions_with` runs this.
pub(in crate::hir::region::infer) fn compute_owned_region_groups(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    order: &Order,
) -> HashMap<HirId, Vec<Region>> {
    // Closure regions, refused below. The group free is the STORE-ONLY-cycle
    // mechanism, and a closure-involving cycle belongs to a different owner
    // (`closure_regions`).
    let closure_regs = closure_regions(hir, info);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    // Region → its allocation HirId: the structural site whose enclosing scope frees
    // the group, and the program-order key for a deterministic member emit-order. A
    // real allocation (`alloc_region`) or a pre-allocated capture cell (keyed by its
    // `Begin`). Each is unique per allocation, so the inversion is well-defined.
    let mut region_alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
    for (&hid, &reg) in &info.alloc_region {
        region_alloc_hir.insert(reg, hid);
    }
    for (&begin_id, cells) in &info.begin_cell_regions {
        for &(_b, reg) in cells {
            region_alloc_hir.insert(reg, begin_id);
        }
    }
    let mut groups: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut claimed: FxHashSet<Region> = FxHashSet::default();
    for &r in &inputs.alloc_regions {
        if claimed.contains(&r) {
            continue;
        }
        // The SCC of `r`: the regions mutually reachable with it (`r ∈ reach(m)` and
        // `m ∈ reach(r)`), from the one shared Tarjan pass (`scc_of`). Size ≥ 2 is a
        // genuine cycle; a lone region (no self-edge) has SCC `{r}` and is skipped. The
        // member the iteration reaches first does not matter, because the SCC set and
        // everything derived from it are the same.
        let scc = inputs.scc_of(r);
        if scc.len() < 2 {
            continue;
        }
        // Every member ownable and sole-held, or the cycle is not Owned at all.
        if scc
            .iter()
            .any(|&m| inputs.not_ownable(info, m) || !inputs.sole_held(m))
        {
            continue;
        }
        // Refuse a closure-involving cycle (`closure_regions` draws the boundary). The
        // closure-cycle MERGE runs before this pass and collapses a mergeable `letrec`
        // clique, and the activation cut claims a capture-back-edge SCC. What lands
        // here is the residual closure cycle no other cut owns, and it stays Shared.
        if scc.iter().any(|&m| closure_regs.contains(&m)) {
            continue;
        }
        // No downstream: the group is exactly the SCC. A cycle that also owns a sub-tree
        // (`reach(SCC) ⊋ SCC`) is refused here (the smallest-cut restriction), staying on
        // the always-legal per-region-RC baseline.
        let reach_scc: FxHashSet<Region> = scc.iter().flat_map(|&m| inputs.reach(m)).collect();
        if reach_scc != *scc {
            continue;
        }
        // Externally unique: nothing outside holds a member (a source SCC).
        if inputs.outside_ref_in(scc) {
            continue;
        }
        // Drop site = the innermost structural scope enclosing every member's allocation
        // (see the fn doc). The scope's `emit_decrefs_for` fires after its whole body,
        // after every in-scope release of a member and of a pass-through alias of one. A
        // member with no allocation site, or members sharing no common structural scope,
        // refuses the group to the always-legal per-region-RC baseline.
        let targets: FxHashSet<HirId> = scc
            .iter()
            .filter_map(|m| region_alloc_hir.get(m).copied())
            .collect();
        if targets.len() != scc.len() {
            continue;
        }
        let Some(drop_site) = innermost_enclosing_scope(hir, &targets) else {
            continue;
        };
        // The drop site frees the group after its whole body, which post-dominates the
        // releases inside that body and no other. A member the scope's value carries out
        // is released by a consumer past the scope, and so is the pass-through result of a
        // within-SCC store that becomes the scope's value. Freeing the group at the scope
        // would free that member under its consumer, so the group refuses to the
        // per-region-RC baseline (docs/impl/region/ownership.md).
        let drop_ord = ord(drop_site);
        let released_inside = |r: &Region| {
            info.region_data.get(r).is_none_or(|d| {
                ord(d.decref_point) <= drop_ord && ord(d.lifetime_point) <= drop_ord
            })
        };
        if !scc.iter().all(released_inside) {
            continue;
        }
        let within_scc_results_inside = info
            .cross_region_refs
            .iter()
            .chain(&info.containment_edges)
            .chain(&info.embed_edges)
            .filter(|&&(_, src, dst)| scc.contains(&src) && scc.contains(&dst))
            .all(|(site, _, _)| info.alloc_region.get(site).is_none_or(released_inside));
        if !within_scc_results_inside {
            continue;
        }
        for &m in scc {
            claimed.insert(m);
        }
        // Merge into the drop-site entry. Two independent cycles sharing one enclosing
        // scope free together, which is sound, because all are dead at scope exit.
        groups
            .entry(drop_site)
            .or_default()
            .extend(scc.iter().copied());
    }
    // Deterministic member order per group, for byte-identical bytecode across compiles.
    // The key is each member's **allocation program order** (`liveness::compute_order`
    // of its allocation site), not the region's raw id. Region ids come from an
    // allocation counter and carry no ordering meaning. Program order is total on the
    // members here: distinct allocations have distinct sites and so distinct positions.
    for members in groups.values_mut() {
        members.sort_by_key(|m| region_alloc_hir.get(m).map(|&h| ord(h)).unwrap_or(0));
    }
    groups
}
