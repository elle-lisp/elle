//! The activation-owner cut: the capture-back-edge SCC
//! (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge SCC").
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
use super::capture::capture_containment_edges;
use super::inputs::ownership_inputs;
use super::subtree::innermost_enclosing_scope;
use rustc_hash::{FxHashMap, FxHashSet};

/// The activation-adopt map the lowerer consumes
/// (`RegionInfo::activation_adopt_sites`): adopt-site HirId — the innermost
/// structural scope enclosing every member's allocation — → the capture-back-edge
/// SCC members to `AdoptIntoActivation` there, in allocation program order.
/// Run by the ownership pass in `analyze_regions_with`.
///
/// Admission (each gate refusing to Shared, the always-legal baseline):
///
/// 1. **The signature.** A genuine mutual-reach SCC (≥ 2 members) of the eligible
///    containment graph whose interior edges include at least one **capture** AND
///    at least one **store** — a non-hard `cross_region_refs` edge, or a
///    funnel-recovered `containment_edges` edge, so the cut admits the checked-on
///    production path (where the store is an opaque `Funnel` call) exactly as it
///    admits the intrinsic path. A capture-only SCC is the letrec closure web
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
///
/// The **free** needs no post-dominance check of its own: the adopt only
/// transfers ownership, and the node's release at the activation's completion is
/// a structural post-dominator of everything in the activation. Pass-through
/// aliases that deref a member after the adopt site read a live `Owned` region
/// (their decrefs are structural no-ops).
pub(in crate::hir::regions) fn compute_activation_adopts(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
    order: &HashMap<HirId, u32>,
) -> HashMap<HirId, Vec<Region>> {
    let inputs = ownership_inputs(hir, info, escape, arena);
    let capture_edges = capture_containment_edges(hir, info, arena);
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
        // The SCC of `r` over the eligible containment graph (mutual reach) —
        // the same SCC computation as the co-owned group walk; which member the
        // iteration reaches first is irrelevant.
        let reach_r = inputs.reach(r);
        let scc: FxHashSet<Region> = reach_r
            .iter()
            .copied()
            .filter(|&m| inputs.reach(m).contains(&r))
            .collect();
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
        let mut hull: FxHashSet<Region> = scc.clone();
        let mut changed = true;
        while changed {
            changed = false;
            for &(_site, s, d) in &info.cross_region_refs {
                if hull.contains(&s) && hull.insert(d) {
                    changed = true;
                }
            }
            for &(_l, s, d) in &capture_edges {
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
        for &m in &scc {
            taken.insert(m);
        }
        out.entry(site).or_default().extend(scc.iter().copied());
    }
    // Deterministic member order per site: allocation program order (region ids
    // order nothing).
    for members in out.values_mut() {
        members.sort_by_key(|m| region_alloc_hir.get(m).map(|&h| ord(h)).unwrap_or(0));
    }
    out
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
