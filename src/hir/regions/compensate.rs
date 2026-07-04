//! Branch-compensation decref placement: free a region on the arms where it dies.
//!
//! The region solver gives every region ONE `decref_point` — the textually-last
//! of its uses. When that point sits inside a conditional arm, a path that takes a
//! DIFFERENT arm reaches the merge without freeing the region — a per-execution
//! leak (pinned by `oracle.lisp`'s branch and `put`/`set` probes). This pass
//! frees the region on those sibling arms, in one of two ways depending on whether
//! the sibling USES the region:
//!
//!  - **dead sibling arm** (no use of `r`): a COMPENSATING release at the arm's
//!    HEAD (`head`). A post-dominator hoist would plug the leak but free `r` LATER
//!    than its true last use on the used arm (an over-keep), and is unsound when an
//!    arm tail-calls — there is no post-branch merge to hoist to, control has
//!    already left the function. The arm head precedes that arm's tail call, so the
//!    release fires before control leaves; the arm has no use of `r`, so head and
//!    the used arm's in-arm decref sit on mutually exclusive paths.
//!  - **used sibling arm** (uses `r`, but the `decref_point` is in a DIFFERENT
//!    arm): a release AFTER `r`'s last use within that arm (`tail`, keyed by that
//!    last-use node and emitted through `emit_decrefs_for`). A head release here
//!    would precede the arm's own use of `r` — a use-after-free. This is the
//!    every-arm-uses-it shape stdlib `put`/`set` take: `(match (type-of coll) …)`
//!    passes the stored value to a different store intrinsic in each arm.
//!
//! Soundness rests on structural facts about region `r` and a branch `C` one of
//! whose arms holds `r`'s `decref_point`:
//!  - **the leak is real and in-arm**: `r`'s `decref_point` is inside an arm of
//!    `C`, so its last use is within `C` (nothing uses `r` after `C`);
//!  - **live-in**: every allocation site of `r` (its `alloc_region` HirIds and
//!    holder-binding def sites) is OUTSIDE `C`'s subtree, so `r` is allocated
//!    before `C` and enters every arm live with RC ≥ 1;
//!  - **loop-invariant guard**: no `While`/`Loop` encloses `C` but not `r`'s
//!    allocation — otherwise `r` is allocated once outside the loop and a
//!    per-iteration arm free would reclaim it mid-loop (a use-after-free, the same
//!    hazard the `Var` iter-scope extension in `liveness/lastuse.rs` guards). When
//!    the alloc IS inside the loop with `C`, the per-iteration free is correct.
//!
//! Regions whose release is owned by another mechanism are excluded, since
//! compensating would double-free: escaping regions (the return frontier projected
//! from escape — the caller frees them), merge children (the root's single decref
//! frees them), co-owned-group members, capture cells, mutated-slot 1-slot
//! containers, and the already-`suppressed_decref_regions`.
//!
//! `head` compensation handles only the two-armed `If` (the dominant `when`/
//! `unless`/`and`/`or`-of-two shape). `tail` compensation handles `If` AND `Match`;
//! `Cond`/`And`/`Or` keep the current behavior until their structure is folded in.

use super::*;
use crate::hir::region::Region;

/// The two per-arm compensation maps. `head[arm_body]` releases at that arm's head
/// (dead sibling arm); `tail[node]` releases after that node — the region's last
/// use within a used sibling arm — through `emit_decrefs_for`.
pub(super) struct BranchComp {
    pub head: HashMap<HirId, Vec<Region>>,
    pub tail: HashMap<HirId, Vec<Region>>,
}

/// A branch (`If` or `Match`) with its whole-node post-order interval and each
/// arm's interval. `is_if` flags the two-armed `If` that `head` compensation is
/// restricted to.
struct Branch {
    is_if: bool,
    node_lo: u32,
    node_hi: u32,
    arms: Vec<(HirId, u32, u32)>,
}

/// A `While`/`Loop`'s post-order subtree interval, for the loop-invariant guard.
struct IterScope {
    lo: u32,
    hi: u32,
}

/// Compute the per-arm compensating decrefs (`head` + `tail`). See the module doc
/// for the soundness conditions. `last_use` maps each use HirId to its consuming
/// (decref-safe) node — the same `compute_last_use` result `decref_point` is
/// computed from, so a `tail` release placed at an arm's max last-use node is the
/// per-arm analogue of the global `decref_point` and is decref-safe by symmetry.
pub(super) fn compute_branch_compensation(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    du: &DefUseBuilder,
    arena: &BindingArena,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
) -> BranchComp {
    // The per-binding source regions are the solver's `binding_source_regions`
    // (== `inference_binding_regions`, already mirrored onto `info` before this
    // runs), so read them from `info` rather than threading a redundant param.
    let binding_regions = &info.binding_source_regions;
    let low = compute_subtree_low(hir, order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    let mut branches: Vec<Branch> = Vec::new();
    let mut loops: Vec<IterScope> = Vec::new();
    collect(hir, order, &low, &mut branches, &mut loops);
    if branches.is_empty() {
        return BranchComp {
            head: HashMap::new(),
            tail: HashMap::new(),
        };
    }

    // Region → its use HirIds and allocation-anchor HirIds, unioned over every
    // holder binding (so an aliased region sees all uses) plus its `alloc_region`
    // sites. A region with no recorded use cannot be analyzed (we don't know where
    // it dies), so it is skipped — the conservative, leak-preserving default.
    let mut region_uses: HashMap<Region, Vec<HirId>> = HashMap::new();
    let mut region_anchors: HashMap<Region, Vec<HirId>> = HashMap::new();
    // How many distinct holder bindings name each region. The `tail` value-route
    // releases through `region_to_slot[r]` — ONE slot. A region named by several
    // bindings (an alias) has several slots but one `region_to_slot` entry, so the
    // per-arm load could target the wrong slot; restrict `tail` to single-holder
    // regions where the slot is unambiguous.
    let mut holder_count: HashMap<Region, u32> = HashMap::new();
    // A region held by a MUTATED (reassigned) or CAPTURED binding cannot be freed
    // by a local-slot value-route: a reassigned slot is repointed over time (the
    // value-route loads whatever it holds NOW, freeing a live later value — the
    // "mutated slot is not a release route" UAF, region-mutable-reassign-param),
    // and a captured value is held cross-region by the closure env (freeing it via
    // the local slot dangles the env reference). Their release is owned by the
    // store / capture-cell path, never this one. The capture fact is the region
    // forest's own reachability question, read from the region capture-graph
    // (`super::escape::captured_bindings`), never the lexical proxy `is_captured`
    // the solver is locked out of; mutation stays a direct structural read.
    let captured = super::escape::captured_bindings(hir);
    let mut tainted: std::collections::HashSet<Region> = std::collections::HashSet::new();
    for (b, regions) in binding_regions {
        let uses = du.uses.get(b);
        let def = du.def_site.get(b);
        let bi = arena.get(*b);
        let unsafe_holder = bi.is_mutated || captured.contains(b);
        for &r in regions {
            *holder_count.entry(r).or_default() += 1;
            if let Some(us) = uses {
                region_uses.entry(r).or_default().extend(us.iter().copied());
            }
            if let Some(&d) = def {
                region_anchors.entry(r).or_default().push(d);
            }
            if unsafe_holder {
                tainted.insert(r);
            }
        }
    }
    for (&alloc_id, &r) in &info.alloc_region {
        region_anchors.entry(r).or_default().push(alloc_id);
    }

    // Regions whose compiler decref is suppressed or owned by another release
    // mechanism — compensating them would double-free. The escaping (returned)
    // regions are the caller's to free; projected from escape's authoritative
    // return verdict (`super::escape`), not a solver-local tail set.
    let tail_regions = super::escape::return_frontier_regions(
        escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );
    let excluded = |r: Region| -> bool {
        info.suppressed_decref_regions.contains(&r)
            || info.owned_group_members.contains(&r)
            || info.cell_release_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || info.merged_root(r) != r
            || tail_regions.contains(&r)
            || tainted.contains(&r)
    };

    // A `tail` per-arm decref is sound ONLY at a node where the value's region is
    // re-incref'd by a STORE on the SAME path — the stored value of `put`/`set`/
    // `push`. The store raises the value's RC (compile-time `IncrefRegion`,
    // unchecked: `cross_region_refs` source = value; or the runtime mutable-store
    // funnel, checked: `funnel_store_sites`), so the live container reference keeps
    // RC ≥ 1 AFTER the per-arm decref: it releases only the value's OWN owning
    // reference (the leak — the temp/arg ref the single `decref_point` frees on just
    // one arm) and can NEVER drop a live value to zero. A node that merely READS the
    // value (a closure CALLED there, a pass-through whose result co-locates in the
    // same region) has no such guard and could over-free, so it keeps the
    // conservative single-`decref_point` baseline. Site-keyed (not mere membership):
    // the store and the decref must be the same node, on one mutually-exclusive arm.
    let mut store_value_at_site: HashMap<HirId, std::collections::HashSet<Region>> = HashMap::new();
    for &(site, src, _) in &info.cross_region_refs {
        store_value_at_site.entry(site).or_default().insert(src);
    }
    for (&site, vals) in &info.funnel_store_sites {
        store_value_at_site
            .entry(site)
            .or_default()
            .extend(vals.iter().copied());
    }

    let mut head: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut tail: HashMap<HirId, Vec<Region>> = HashMap::new();
    for (&r, uses) in &region_uses {
        if uses.is_empty() || excluded(r) {
            continue;
        }
        let Some(d) = info.region_data.get(&r).map(|rd| ord(rd.decref_point)) else {
            continue;
        };
        let anchors = match region_anchors.get(&r) {
            Some(a) => a,
            None => continue,
        };
        for br in &branches {
            // Which arm holds the decref_point? (The leak is real only when the
            // release lands inside an arm — an extension that hoisted it out, or a
            // post-dominating point, leaves `d` outside every arm here.)
            let arm_of_d = br.arms.iter().position(|&(_, lo, hi)| d >= lo && d <= hi);
            let Some(di) = arm_of_d else { continue };
            // live-in: every anchor is outside C's subtree.
            let live_in = anchors
                .iter()
                .all(|&a| ord(a) < br.node_lo || ord(a) > br.node_hi);
            if !live_in {
                continue;
            }
            // loop-invariant guard: no loop encloses C but not r's allocation.
            let crosses_loop = loops.iter().any(|l| {
                let c_in = l.lo <= br.node_lo && br.node_hi <= l.hi;
                c_in && anchors.iter().any(|&a| ord(a) < l.lo || ord(a) > l.hi)
            });
            if crosses_loop {
                continue;
            }
            for (ai, &(arm_id, arm_lo, arm_hi)) in br.arms.iter().enumerate() {
                if ai == di {
                    continue; // the arm whose in-arm decref is the global decref_point
                }
                // r's last-use nodes for uses within this sibling arm. Both the use
                // AND its consuming (last-use) node must lie inside the arm: the
                // decref is emitted at the node (through `emit_decrefs_for`), so a
                // node OUTSIDE the arm would not be mutually exclusive with the
                // `decref_point` arm — a double-free on the shared path. (ANF can
                // float a use's consumer past its own arm; filter on the node, not
                // just the use.)
                let arm_last_use = uses
                    .iter()
                    .filter(|&&u| {
                        let o = ord(u);
                        o >= arm_lo && o <= arm_hi
                    })
                    .map(|&u| last_use.get(&u).copied().unwrap_or(u))
                    .filter(|&n| {
                        let o = ord(n);
                        o >= arm_lo && o <= arm_hi && o != d
                    })
                    .max_by_key(|&n| ord(n));
                match arm_last_use {
                    // Used sibling arm: release after its last use of r. Keyed on
                    // the last-use node, emitted through `emit_decrefs_for` (which
                    // fires AFTER that node), never at the arm head (which precedes
                    // the use). `d` is in a different arm, so `node != d`.
                    //
                    // Restricted to `call_result_regions` — a per-call value whose
                    // release is the value-route (`LoadLocal` slot + `DecrefValueRegion`
                    // off the slot that still holds it in this arm), the exact target
                    // class (a fresh string / struct stored in each arm). A
                    // non-call-result region releases by static slot
                    // (`DecrefRegion`, which CLEARS the activation slot), whose
                    // cross-arm interaction is not value-route-safe; such a region
                    // keeps the conservative single-`decref_point` baseline
                    // (leak-preserving, never a double-free).
                    Some(node)
                        if info.call_result_regions.contains(&r)
                            && holder_count.get(&r).copied().unwrap_or(0) == 1
                            && store_value_at_site
                                .get(&node)
                                .is_some_and(|s| s.contains(&r)) =>
                    {
                        tail.entry(node).or_default().push(r)
                    }
                    Some(_) => {}
                    // Dead sibling arm (no use): head release, for the two-armed
                    // `If` only (the existing leak-tested shape).
                    None if br.is_if => head.entry(arm_id).or_default().push(r),
                    None => {}
                }
            }
        }
    }
    // Deterministic per-node order (region id), independent of hash iteration.
    for regions in head.values_mut().chain(tail.values_mut()) {
        regions.sort_by_key(|r| r.0);
        regions.dedup();
    }
    BranchComp { head, tail }
}

/// Collect every `If`/`Match` (with its arms' intervals) and every `While`/`Loop`
/// (with its subtree interval) in one walk.
fn collect(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    branches: &mut Vec<Branch>,
    loops: &mut Vec<IterScope>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = |id: HirId| low.get(&id).copied().unwrap_or(0);
    match &hir.kind {
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            branches.push(Branch {
                is_if: true,
                node_lo: lo(hir.id),
                node_hi: ord(hir.id),
                arms: vec![
                    (then_branch.id, lo(then_branch.id), ord(then_branch.id)),
                    (else_branch.id, lo(else_branch.id), ord(else_branch.id)),
                ],
            });
        }
        HirKind::Match { arms, .. } => {
            branches.push(Branch {
                is_if: false,
                node_lo: lo(hir.id),
                node_hi: ord(hir.id),
                arms: arms
                    .iter()
                    .map(|(_pat, _guard, body)| (body.id, lo(body.id), ord(body.id)))
                    .collect(),
            });
        }
        HirKind::While { .. } | HirKind::Loop { .. } => {
            loops.push(IterScope {
                lo: lo(hir.id),
                hi: ord(hir.id),
            });
        }
        _ => {}
    }
    hir.for_each_child(|c| collect(c, order, low, branches, loops));
}
