// audited: 2026-10-20
//! The ownership cuts that free a whole subtree under one root, and a rootless cycle as one group.
//!
//! docs/impl/region/ownership.md
//! docs/impl/region/adopt.md

use super::super::*;
use super::capture::closure_regions;
use super::inputs::OwnershipInputs;
use rustc_hash::{FxHashMap, FxHashSet};

/// The externally-unique Owned subtrees of a compilation unit — the consumer of
/// `compute_shared_seeds`. Maps each Owned **root** region to the
/// full set of regions in its subtree (the root included). A region absent from every
/// value here is Shared (the conservative baseline: per-region RC, always legal).
///
/// ## What a subtree is — the containment graph, read outward
///
/// The edges are `RegionInfo::cross_region_refs`: each `(site, source, target)` means
/// a value in `source` is stored into a structure in `target`, so at runtime `target`
/// holds a reference to `source` and the free-time cascade frees `source` when
/// `target` dies (region.rs). In ownership terms **`target` is the parent and
/// `source` the child** — the subtree rooted at a container `r` is `r` plus every
/// region transitively *contained in* it, reached by following edges from a parent
/// already in the set to its children (`target ∈ S ⇒ add source`).
///
/// Only **eligible** edges build the subtree: an edge recorded at a `hard_edge_site`
/// is a native may-store/clique over-approximation (`Mixed`/`Unknown`/declared
/// `Stores`), and ownership cannot be proven *through* a may-store, so such edges do
/// not extend `Reach`. (They still count *against* external uniqueness — see below.)
///
/// The graph also carries **capture edges** (`capture_containment_edges`): a closure's
/// region contains every value it captures, but capture records *no* `cross_region_refs`
/// edge (the runtime counts a closure's env itself — see the module doc), so they are
/// re-derived here from the HIR. They are eligible (a capture is a genuine containment, not a may-store) and
/// — like every edge — count against external uniqueness.
///
/// And the **funnel-store and cell containment** (`RegionInfo::containment_edges`): a
/// value stored into a mutable retaining container (`@array`/`@struct`) by a `Funnel`
/// op, recovered from the container's `RetType` on the production native-call path
/// where the funnel records no `cross_region_refs` edge (module doc, source 3), and a
/// compiled capture cell's content. Eligible (genuine containment) and counted against
/// external uniqueness, exactly like the capture edges.
///
/// The **embed edges** (`RegionInfo::embed_edges`, a `Fresh` native's declared
/// `result ⊇ arg`) are read one way only: they count against external uniqueness and
/// build no subtree. There is no site at which the result could adopt the argument,
/// so a subtree built through the edge would refuse at the single-owner filter and
/// take every container holding the result down with it
/// (docs/impl/region/effects.md).
///
/// ## When a subtree is Owned — external uniqueness
///
/// `S = {r} ∪ Reach(r)` is externally unique, and therefore Owned, iff:
///
/// 1. **No region in `S` crosses a frontier or has runtime-determined lifetime.** No
///    region in `S` is a Shared seed (`compute_shared_seeds`: return / emit / send) and
///    none is one of the dynamic classes that are never statically ownable —
///    `call_result_regions` (a callee's result region, possibly a borrow),
///    `cell_release_regions`, `mutated_binding_value_regions`,
///    `suppressed_decref_regions` (the runtime-counted reassign-cell values). Any such
///    member means subtree drop could free a value that outlives `S` or whose identity
///    the compiler cannot bound → refuse.
/// 2. **Nothing outside `S` references into `S`.** No edge `(_, source, target)` — over
///    every edge kind: *all* `cross_region_refs`, hard edges included, the capture
///    edges, the containment edges and the embed edges — has `source ∈ S` and
///    `target ∉ S`. Such an edge is an outside container holding a reference to an
///    interior region; subtree-dropping `S` would dangle it. (The root's own single
///    reference is the activation/binding that sole-holds it, which is not a
///    `cross_region_refs` edge, so it is allowed by construction.) This is the check
///    that catches region-level aliasing (a child stored into two parents) and a
///    may-store from outside.
///
/// A **candidate root** is a live region that is sole-held (≤1 distinct user binding,
/// via `RegionHolders`) and is a *top container* — not the `source` of any eligible
/// edge, i.e. not itself contained in another region. Restricting roots to top
/// containers yields maximal, non-overlapping subtrees: an interior child is reached
/// from its parent, and a child stored into two parents fails check 2 for both, so the
/// accepted roots partition disjoint subtrees.
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
        // Candidate root: a sole-held, non-frontier TOP container (not the child of any
        // eligible edge — a rootless cycle, whose members are all `contained`, is the
        // co-owned-cycle walk's job, `compute_owned_region_groups`).
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

/// The co-owned region **groups** — the co-owned-cycle cut. A mutual
/// reference cycle with no container parent (an externally-unique *source* strongly-
/// connected component of the containment graph — e.g. `(let [a (@array) b (@array)]
/// (push a b) (push b a))`) has no owner among its members: each owns and is owned by
/// the others. It is therefore reclaimed **symmetrically as one unit** rather than by
/// promoting any member to root (which would force an arbitrary, lifetime-meaningless
/// choice). Maps each group's **drop site** — the innermost structural scope
/// ([`innermost_enclosing_scope`]) lexically enclosing every member's allocation — to its
/// member regions, where the lowerer emits one `FreeRegionGroup` over the whole set in
/// place of the members' individual decrefs.
///
/// The drop site is the enclosing *scope*, **not** a comparison of the members'
/// `decref_point`s: a member's region is also dereferenced by every **pass-through alias**
/// of it. `%array-push`/`%put` return their container, so a discarded store result's
/// `result_region_of` derefs a member's region one structural step *past* the member's own
/// `decref_point`. Freeing the set at `max(member decref_point)` frees those regions
/// before the alias deref — a stale-deref UAF. The enclosing scope node post-dominates
/// the whole body — hence every in-scope pass-through alias release — so the group free
/// lands after all of them. No member `decref_point` is compared to pick the site:
/// numeric/order identity is not the lifetime ordering, structural nesting is.
///
/// A group qualifies iff its members form an SCC of size ≥ 2 (a genuine cycle), every
/// member is ownable and sole-held, the SCC owns no downstream members
/// (`reach(SCC) == SCC` — the smallest-cut restriction; a cycle that also owns a sub-tree
/// stays on the always-legal per-region-RC baseline), it is externally unique
/// (`outside_ref_in` false — equivalently a SOURCE in the condensation: no external
/// container holds a member), and every member and within-SCC store result is released
/// inside the drop site. The scope's value can carry a member out to a consumer past
/// the scope, and the scope's end does not post-dominate that consumer's read.
///
/// Disjoint from [`compute_owned_subtrees`] by construction: a source SCC is reached by
/// no top container (being reached would mean an external container holds a member, i.e.
/// not a source), so the container-rooted subtrees and the co-owned groups never overlap.
/// Run by the ownership pass in `analyze_regions_with`.
pub(in crate::hir::region::infer) fn compute_owned_region_groups(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    order: &Order,
) -> HashMap<HirId, Vec<Region>> {
    // Closure regions — refused below: the group free is the STORE-ONLY-cycle mechanism, and
    // a closure-involving cycle belongs to a different owner. A `letrec` cell↔closure clique
    // goes to the closure-cycle MERGE (which collapses it to one arena); a capture-back-edge
    // SCC to the activation cut. The group free resolves members with `result_region_of`
    // (which unwraps a `CaptureCell`), so it cannot name a cell member's own region — the
    // boundary this refusal draws (`closure_regions` doc).
    let closure_regs = closure_regions(hir, info);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    // Region → its allocation HirId — the structural site whose enclosing scope frees the
    // group, and (below) the program-order key for a deterministic member emit-order. A
    // real allocation (`alloc_region`) or a pre-allocated capture cell (keyed by its
    // `Begin`); unique-per-alloc, so the inversion is well-defined.
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
        // `m ∈ reach(r)`). Read from the one shared Tarjan pass (`scc_of`). Size ≥ 2 is a
        // genuine cycle; a lone region (no self-edge) has SCC `{r}` and is skipped. Which
        // member of an SCC the iteration reaches first is irrelevant — the SCC set, and
        // everything derived from it, is the same.
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
        // Refuse a closure-involving cycle: the group free is the store-only-cycle
        // instrument, and it resolves each member with `result_region_of` (which unwraps a
        // `CaptureCell`), so it cannot name a cell member's own region. A mergeable `letrec`
        // cell↔closure clique is collapsed by the closure-cycle MERGE (`region::infer::merge`, run
        // before this pass, resolving through `merged_root`) and never reaches the group walk;
        // a capture-back-edge SCC is claimed by the activation cut. What lands here and is
        // refused is the residual closure cycle no other cut owns — it stays Shared (the
        // always-legal baseline). See `closure_regions` for the mechanism boundary.
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
        // Merge into the drop-site entry (two independent cycles sharing one enclosing
        // scope free together — sound, all dead at scope exit). Sorted once below.
        groups
            .entry(drop_site)
            .or_default()
            .extend(scc.iter().copied());
    }
    // Deterministic member order per group, for byte-identical bytecode across compiles.
    // The key is each member's **allocation program order** (`compute_order` of its alloc
    // site) — a structurally-meaningful position, not the region's raw numeric identity:
    // region ids are an allocation-counter artifact and carry no ordering semantics, so
    // comparing them to order anything is overloading comparison onto a non-order. Program
    // order is a total order on the members here (unique-per-alloc → distinct sites →
    // distinct `order` values).
    for members in groups.values_mut() {
        members.sort_by_key(|m| region_alloc_hir.get(m).map(|&h| ord(h)).unwrap_or(0));
    }
    groups
}

/// The innermost structural scope node (`Let`/`Letrec`/`Begin`/`Loop`/`Block`) that
/// lexically encloses every HirId in `targets`, or `None` if they share no common
/// structural scope within one function body. This is the co-owned-cycle cut's drop
/// point ([`compute_owned_region_groups`]): the lowerer's `emit_decrefs_for` on this
/// node fires after its whole body, so a `FreeRegionGroup` emitted there post-dominates
/// every member's last use and every pass-through-alias release inside the scope.
///
/// Computed as the deepest node common to every target's structural-scope ancestor stack
/// (the longest common prefix). A `Lambda` **resets** the stack — its body is a separate
/// function, so a scope outside the lambda cannot enclose a node inside it; a group whose
/// members live directly in a lambda body with no enclosing structural scope returns
/// `None` and stays Shared (the always-legal baseline). The scope node's own id is not in
/// its descendants' stacks (recorded at entry, before the node is pushed), which is what
/// makes the result a strict *ancestor* — and thus a post-dominator in post-order.
pub(in crate::hir::region::infer) fn innermost_enclosing_scope(
    hir: &Hir,
    targets: &FxHashSet<HirId>,
) -> Option<HirId> {
    fn is_scope(k: &HirKind) -> bool {
        matches!(
            k,
            HirKind::Let { .. }
                | HirKind::Letrec { .. }
                | HirKind::Begin(_)
                | HirKind::Loop { .. }
                | HirKind::Block { .. }
        )
    }
    fn walk(
        h: &Hir,
        targets: &FxHashSet<HirId>,
        stack: &mut Vec<HirId>,
        found: &mut Vec<Vec<HirId>>,
    ) {
        if targets.contains(&h.id) {
            found.push(stack.clone());
        }
        if matches!(h.kind, HirKind::Lambda { .. }) {
            // Separate function body — a fresh ancestor stack (outer scopes do not enclose
            // it). All members of one co-owned cycle are allocated in one activation, so
            // a cross-lambda common prefix would be empty (→ refuse), which is correct.
            let mut inner = Vec::new();
            h.for_each_child(|c| walk(c, targets, &mut inner, found));
            return;
        }
        let pushed = is_scope(&h.kind);
        if pushed {
            stack.push(h.id);
        }
        h.for_each_child(|c| walk(c, targets, stack, found));
        if pushed {
            stack.pop();
        }
    }
    let mut found: Vec<Vec<HirId>> = Vec::new();
    walk(hir, targets, &mut Vec::new(), &mut found);
    // Every target must have been located, or the result would be a scope enclosing only
    // some of them — refuse.
    if found.len() != targets.len() {
        return None;
    }
    let first = found.first()?;
    let mut common = first.len();
    for s in &found[1..] {
        let mut i = 0;
        while i < common && i < s.len() && s[i] == first[i] {
            i += 1;
        }
        common = i;
    }
    (common > 0).then(|| first[common - 1])
}
