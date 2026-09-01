//! The activation-owner cut: the capture-back-edge SCC
//! (docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge SCC").
//!
//! A container captured by a closure it holds (`m ⊇ c` by store, `c ⊇ m` by
//! capture — the m↔c SCC) is the one containment-graph cycle neither
//! region-rooted mode can own: the owner-aware lifetime obligation refuses a
//! captured-AND-store-adopted member (its own live `DecrefValueRegion` fires at
//! the over-extended position past the closure, after any region root's drop —
//! `compute_adopt_edges`), and the co-owned group free refuses any cycle
//! touching a closure region (`closure_regions`). The owner that CAN reclaim it
//! is the **executing activation**: its owner node's completion release
//! post-dominates every in-activation use by construction (the trampoline's
//! clean break, the parks, the discard, and the fiber teardown all landed with
//! the owner-node runtime), so adopting the SCC's members into the node and
//! suppressing their own decrefs frees the cycle wholesale — interior m↔c
//! references reclaim with the set, no cascade.

use super::super::*;
use super::inputs::OwnershipInputs;
use super::subtree::innermost_enclosing_scope;
use rustc_hash::{FxHashMap, FxHashSet};

/// The activation-adopt map the lowerer consumes
/// (`RegionInfo::activation_adopt_sites`): adopt-site HirId → the
/// capture-back-edge SCC members to `AdoptIntoActivation` there, in allocation
/// program order. The primary site is the innermost structural scope enclosing
/// every member's allocation; a scope that can park carries a SECOND entry for
/// the same members — an early key on the scope's sequential spine, ahead of
/// every park (gate 6, the park split). The channel is idempotent on an Owned
/// member, so the scope-exit adopt behind the early key is a structural no-op
/// on the paths that ran both. Run by the ownership pass in
/// `analyze_regions_with`.
///
/// Admission (each gate refusing to Shared, the always-legal baseline):
///
/// 1. **The signature.** A genuine mutual-reach SCC (≥ 2 members) of the eligible
///    containment graph whose interior edges include at least one **capture** AND
///    at least one **store** — a non-hard `cross_region_refs` edge, or a
///    funnel-recovered `containment_edges` edge (the storing ops lower as opaque
///    `Funnel` native calls), so the cut admits the funnel store face exactly as
///    it admits an intrinsic-store edge. A capture-only SCC is the letrec closure web
///    (the closure-cycle merge's instrument); a store-only SCC is the co-owned
///    group's.
/// 2. **Member gates.** Every member ownable (`not_ownable` false — no frontier
///    crossing, no dynamic-lifetime class), sole-held, with an allocation site,
///    and with **pairwise-distinct sole holder bindings**: the value-resolved
///    adopt loads each member from its own binding slot, so two members sharing
///    a holder (a branch-dependent union in one slot) cannot both be adopted —
///    the shape refuses rather than leak a suppressed-but-unadopted member.
/// 3. **Disjointness.** No member is claimed by another mechanism — a merge
///    participant (builder-idiom or closure-cycle), a co-owned group member, or
///    a store/capture-adopt subtree region (either endpoint) — the one-owner
///    invariant held at the emit level, so the runtime's one-adoption assert
///    stays unreachable.
/// 4. **The hull.** Every region referencing INTO the SCC, transitively over ALL
///    edge kinds (hard may-store edges included — a may-holder is a holder),
///    must itself be ownable: the members free at the activation's completion,
///    so every holder must provably die within the activation. A holder that
///    returns, crosses a fiber frontier, or has runtime-determined lifetime
///    refuses the SCC. Hull members outside the SCC keep their own baseline
///    releases — their cascades onto the Owned members are structural no-ops.
/// 5. **One activation, no loop seam.** The members' allocation sites share an
///    innermost enclosing structural scope — the adopt site
///    ([`innermost_enclosing_scope`]; a cross-lambda SCC refuses) — and no
///    `While`/`Loop` encloses a member's allocation without also enclosing that
///    site. Adopt-per-iteration is sound (each iteration adopts that round's
///    fresh regions); alloc-inside/adopt-outside is not — the static suppression
///    covers every iteration while the slot-loaded adopt reaches only the last.
/// 6. **The park split.** No park may run between a member's allocation and its
///    adopt: in that interval the members are `Counted` with their own releases
///    suppressed, so a route that abandons such a park strands the whole SCC —
///    no release table names the members, and no node adopted them. When the
///    adopt scope can park (`Signal::may_park` on its aggregated signal), the
///    walk finds an early adopt key on the scope's sequential spine
///    ([`park_split_key`]) and emits the adopt there too; a shape the spine
///    cannot order refuses to Shared
///    (docs/impl/region/owner.md § "Owner nodes" — "The park split").
///
/// The **free** needs no post-dominance check of its own: the adopt only
/// transfers ownership, and the node's release at the activation's completion is
/// a structural post-dominator of everything in the activation. Pass-through
/// aliases that deref a member after the adopt site read a live `Owned` region
/// (their decrefs are structural no-ops).
pub(in crate::hir::region::infer) fn compute_activation_adopts(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    order: &HashMap<HirId, u32>,
) -> HashMap<HirId, Vec<Region>> {
    let capture_edges = inputs.capture_edges();
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    // Region → allocation site (real allocations + prebound capture cells), the
    // structural key for the adopt site and the program-order member sort — as in
    // the co-owned group walk.
    let mut region_alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
    for (&hid, &reg) in &info.alloc_region {
        region_alloc_hir.insert(reg, hid);
    }
    for (&begin_id, cells) in &info.begin_cell_regions {
        for &(_b, reg) in cells {
            region_alloc_hir.insert(reg, begin_id);
        }
    }

    // Disjointness (gate 3): regions already claimed by another mechanism.
    let mut other_claimed: FxHashSet<Region> = FxHashSet::default();
    for (&child, &parent) in &info.merged_parent {
        other_claimed.insert(child);
        other_claimed.insert(parent);
    }
    other_claimed.extend(info.closure_cycle_members.iter().copied());
    other_claimed.extend(info.owned_group_members.iter().copied());
    for edges in info
        .owned_adopt_edges
        .values()
        .chain(info.capture_adopt_edges.values())
    {
        for &(member, owner) in edges {
            other_claimed.insert(member);
            other_claimed.insert(owner);
        }
    }

    // Loop intervals `[low, order]` for the loop-seam gate (gate 5).
    let low = compute_subtree_low(hir, order);
    let mut loops: Vec<(u32, u32)> = Vec::new();
    collect_loop_intervals(hir, order, &low, &mut loops);

    let mut out: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut taken: FxHashSet<Region> = FxHashSet::default();
    for &r in &inputs.alloc_regions {
        if taken.contains(&r) {
            continue;
        }
        // The SCC of `r` over the eligible containment graph (mutual reach) — read from the
        // one shared Tarjan pass (`scc_of`), the same SCC the co-owned group walk reads;
        // which member the iteration reaches first is irrelevant.
        let scc = inputs.scc_of(r);
        if scc.len() < 2 {
            continue;
        }
        // Gate 1 — the capture-back-edge signature: interior capture AND store.
        let has_capture = capture_edges
            .iter()
            .any(|&(_l, s, d)| scc.contains(&s) && scc.contains(&d));
        let has_store = info.cross_region_refs.iter().any(|&(site, s, d)| {
            !info.hard_edge_sites.contains(&site) && scc.contains(&s) && scc.contains(&d)
        }) || info
            .containment_edges
            .iter()
            .any(|&(_site, s, d)| scc.contains(&s) && scc.contains(&d));
        if !has_capture || !has_store {
            continue;
        }
        // Gates 2 + 3 — member gates and disjointness.
        if scc.iter().any(|&m| {
            inputs.not_ownable(info, m) || !inputs.sole_held(m) || other_claimed.contains(&m)
        }) {
            continue;
        }
        let holders: Vec<Binding> = scc.iter().filter_map(|&m| inputs.sole_holder(m)).collect();
        let distinct: FxHashSet<Binding> = holders.iter().copied().collect();
        if holders.len() != scc.len() || distinct.len() != scc.len() {
            continue;
        }
        // Gate 4 — the hull: every transitive holder of a member, over ALL edge
        // kinds, must itself be ownable (die within the activation).
        let mut hull: FxHashSet<Region> = (*scc).clone();
        let mut changed = true;
        while changed {
            changed = false;
            for &(_site, s, d) in &info.cross_region_refs {
                if hull.contains(&s) && hull.insert(d) {
                    changed = true;
                }
            }
            for &(_l, s, d) in capture_edges {
                if hull.contains(&s) && hull.insert(d) {
                    changed = true;
                }
            }
            for &(_site, s, d) in &info.containment_edges {
                if hull.contains(&s) && hull.insert(d) {
                    changed = true;
                }
            }
        }
        if hull
            .iter()
            .any(|&h| !scc.contains(&h) && inputs.not_ownable(info, h))
        {
            continue;
        }
        // Gate 5 — one activation (the adopt site), no loop seam.
        let targets: FxHashSet<HirId> = scc
            .iter()
            .filter_map(|m| region_alloc_hir.get(m).copied())
            .collect();
        if targets.len() != scc.len() {
            continue;
        }
        let Some(site) = innermost_enclosing_scope(hir, &targets) else {
            continue;
        };
        let site_ord = ord(site);
        let loop_seam = targets.iter().any(|&a| {
            let ao = ord(a);
            loops
                .iter()
                .any(|&(lo, hi)| lo <= ao && ao <= hi && !(lo <= site_ord && site_ord <= hi))
        });
        if loop_seam {
            continue;
        }
        // Gate 6 — the park split: a parking adopt scope gets an early key
        // ahead of every park, or refuses. The scope-exit entry stays beside
        // the early one — a path that leaves the scope before the key (a
        // `break` past it) still adopts there, and the channel's idempotence
        // absorbs the double adopt on paths that run both.
        let mut early_key = None;
        if let Some(site_node) = find_node(hir, site) {
            if site_node.signal.may_park() {
                match park_split_key(site_node, &targets, false) {
                    Some(k) => early_key = Some(k),
                    None => continue,
                }
            }
        }
        for &m in scc {
            taken.insert(m);
        }
        out.entry(site).or_default().extend(scc.iter().copied());
        if let Some(k) = early_key {
            out.entry(k).or_default().extend(scc.iter().copied());
        }
    }
    // Deterministic member order per site: allocation program order (region ids
    // order nothing).
    for members in out.values_mut() {
        members.sort_by_key(|m| region_alloc_hir.get(m).map(|&h| ord(h)).unwrap_or(0));
    }
    out
}

/// The node with `id` in `hir`'s tree, by structural search — the adopt site's
/// `&Hir` for the park-split gate (gate 6), which reads its aggregated signal
/// and its spine.
fn find_node(hir: &Hir, id: HirId) -> Option<&Hir> {
    if hir.id == id {
        return Some(hir);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_node(c, id);
        }
    });
    found
}

/// The node's sequential constituents in execution order — a `Let`/`Letrec`'s
/// binding inits then body, a `Begin`/`Block`'s statements — each completing
/// before the next begins, so `emit_decrefs_for` on a constituent's id runs
/// between it and its successor (for a binding init, after the binding's
/// store). `None` for any other node: its interior execution order is not one
/// this walk can read, so the park split refuses rather than guess.
fn sequential_constituents(node: &Hir) -> Option<Vec<&Hir>> {
    match &node.kind {
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => Some(
            bindings
                .iter()
                .map(|(_b, init)| init)
                .chain(std::iter::once(&**body))
                .collect(),
        ),
        HirKind::Begin(items) => Some(items.iter().collect()),
        HirKind::Block { body, .. } => Some(body.iter().collect()),
        _ => None,
    }
}

/// Does `hir`'s subtree contain any of `targets` (the members' allocation
/// sites)?
fn contains_any(hir: &Hir, targets: &FxHashSet<HirId>) -> bool {
    if targets.contains(&hir.id) {
        return true;
    }
    let mut found = false;
    hir.for_each_child(|c| {
        if !found {
            found = contains_any(c, targets);
        }
    });
    found
}

/// The park split (gate 6): the node whose `emit_decrefs_for` is the early
/// adopt key — ordered after every member allocation and before every park
/// that could strand one — or `None` where no such node exists on the
/// sequential spine.
///
/// Per spine level, the key candidate is the LAST constituent containing a
/// member allocation: every member's binding store completes with it, and
/// every later constituent's park finds the members already Owned. A park in
/// an EARLIER constituent refuses only where a member allocation precedes it
/// (`allocs_before` carries that fact into recursion) — a park before every
/// member allocation strands nothing, because no member exists when it parks.
/// When the candidate itself both allocates and parks, the walk recurses into
/// it; a non-sequential candidate ([`sequential_constituents`] `None`) or a
/// member allocated outside every constituent refuses.
fn park_split_key(node: &Hir, targets: &FxHashSet<HirId>, allocs_before: bool) -> Option<HirId> {
    let cs = sequential_constituents(node)?;
    let has_alloc: Vec<bool> = cs.iter().map(|c| contains_any(c, targets)).collect();
    let last_a = has_alloc.iter().rposition(|&a| a)?;
    for (i, c) in cs.iter().enumerate() {
        if i >= last_a || !c.signal.may_park() {
            continue;
        }
        if allocs_before || has_alloc[..=i].iter().any(|&a| a) {
            return None;
        }
    }
    if cs[last_a].signal.may_park() {
        let inner_before = allocs_before || has_alloc[..last_a].iter().any(|&a| a);
        park_split_key(cs[last_a], targets, inner_before)
    } else {
        Some(cs[last_a].id)
    }
}

/// Every `While`/`Loop` node's post-order subtree interval `[low, order]`, so
/// "a loop encloses this position" is an interval test — the loop-seam gate's
/// input (gate 5).
fn collect_loop_intervals(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<(u32, u32)>,
) {
    if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
        let lo = low.get(&hir.id).copied().unwrap_or(0);
        let hi = order.get(&hir.id).copied().unwrap_or(0);
        out.push((lo, hi));
    }
    hir.for_each_child(|c| collect_loop_intervals(c, order, low, out));
}
