// audited: 2026-09-14
//! The branch-arm release window: moving a release that landed inside one arm
//! onto the branch, where every path arrives.
//!
//! docs/impl/region/window.md

use super::super::super::*;
use super::branchscopes::{alias_binders, collect_branch_scopes, BranchWindowScopes, FrameExit};
use crate::hir::defuse::DefUseBuilder;

/// Re-anchor a release that landed inside one arm of a branch onto the branch.
///
/// A region's `decref_point` is the structurally-latest of its uses. When several
/// arms use it, "latest" resolves to a node inside ONE arm — and arms are
/// mutually exclusive, so every execution taking a different arm emits no release
/// at all and holds the whole region (plus every member its free cascade would
/// reclaim) to fiber teardown. "Latest across the arms" is not a point any single
/// execution passes through.
///
/// The point every execution does pass through is `last_use[branch]` — the node
/// consuming the branch's value, or the branch itself when nothing does — whose
/// decrefs the lowerer emits after the merge label. Moving the release there is a
/// placement argument, the one the break window makes: one release still, landing
/// later (docs/impl/region/mechanism.md § "A release inside one arm is not a
/// release on the other arms").
///
/// Placement is enough only where this frame **holds the region alone** —
/// the release fires on arms where none did before, and the other holder within
/// reach is an uncounted borrow in a parked frame. That is escape's question, and
/// the admission below is its answer; everything escape cannot clear keeps the
/// baseline and the counted per-arm routes.
///
/// The **return** facet is no refusal here. The merge is a point in this frame, so
/// every mint the taken arm ran has already fired there; and a replica ahead of an
/// arm's `TailCall`, which does precede its callee's mint, owes no edge either — a
/// callee reaches a value this frame owns as an operand or through its captured
/// environment and by no other route, so it either counts the region or cannot mint
/// against it (region/mechanism.md § "The callee's return mint, and why the point
/// owes it nothing").
///
/// Once a region's `decref_point` leaves the arms, `region::infer::compensate` no
/// longer finds it inside one, so neither of its per-arm routes fires for that
/// region — the single anchored release is what they were approximating.
///
/// Two boundaries bound the window, the same two the break window carries, and
/// both are about how many times a release runs: a `While`/`Loop` nested in the
/// branch and holding the `decref_point` (its body re-allocates per iteration, so
/// one release cannot cover N) and a `Lambda` holding it (its releases run in
/// another activation, against another frame's slots). Each is the scope's BODY,
/// never the scope's own node — a release anchored at the loop node already runs
/// once per execution of the loop.
///
/// An arm that leaves through a **frame-replacing** tail call does not arrive at
/// the merge, so the anchor alone does not cover it — the frame-exit relocation
/// does, replicating the anchored release ahead of that arm's `TailCall` or
/// leaving it to the callee that took the argument over. That replica exists only
/// for a value-routed release, so such a branch narrows the window to the regions
/// a value route can NAME (`value_routed`, `region::infer::escape`) instead of
/// declining whole; everything else keeps its in-arm release and
/// `region::infer::compensate`'s counted routes. A tail call to a *native* pushes
/// no frame and falls through, which is why the callee kind decides it
/// (`frame_replacing_tail_calls`) and not `is_tail`.
///
/// The exemption the relocation already reads (`TailExitHoist::exempt`) decides one
/// more refusal, because its two halves owe different things. An **argument** the
/// call names is the ownership move — the callee's owned-parameter release runs in
/// place of the copy the arm left in its dead block — so the anchor may take that
/// release away. The **callee's own** region has no such release standing in for
/// it: the deferred callee channel does, and that channel is keyed on where the
/// release sits, so anchoring it at the merge leaves the exiting arm with nothing
/// (region/mechanism.md § "What the exemption keeps, a channel must still run").
///
/// The region must also be **live-in** — every allocation and holder-definition
/// site outside the branch's subtree — so a value born inside an arm keeps its
/// in-arm release and the window only moves what the branch received.
#[allow(clippy::too_many_arguments)]
pub(super) fn pin_branch_arm_releases(
    info: &mut RegionInfo,
    hir: &Hir,
    du: &DefUseBuilder,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    frame_replacing_tail_calls: &rustc_hash::FxHashSet<HirId>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let low = compute_subtree_low(hir, order);
    let mut scopes = BranchWindowScopes::default();
    collect_branch_scopes(
        hir,
        info,
        order,
        &low,
        frame_replacing_tail_calls,
        &mut scopes,
    );
    if scopes.branches.is_empty() {
        return;
    }
    // Inner branches first: a release hoisted to an inner branch's anchor is
    // still inside the enclosing arm, so the outer branch can carry it the rest
    // of the way. Post-order indexes a child below its parent, so ascending
    // `node_hi` is exactly innermost-outward.
    scopes.branches.sort_by_key(|b| b.node_hi);

    // The live-in premise's anchors: every allocation site of each region, and the
    // definition site of every holder binding that could be the release's ROUTE.
    // `record_region_slot` keys `region_to_slot` on a region's ALLOCATION site, so
    // the slot a value-routed release loads belongs to the binding whose init
    // allocated the region — or, for a region no site in this body allocates, to
    // the parameter the lambda prologue recorded. An ALIAS binder, whose init
    // merely names another binding, records no slot and can never be the route, so
    // its definition site anchors nothing: an arm that introduces one has not given
    // birth to the value and the branch still received it.
    let aliases = alias_binders(hir);
    let mut region_anchors: HashMap<Region, Vec<u32>> = HashMap::new();
    for (b, regions) in inference_binding_regions {
        if aliases.contains(b) {
            continue;
        }
        if let Some(&d) = du.def_site.get(b) {
            for &r in regions {
                region_anchors.entry(r).or_default().push(ord(d));
            }
        }
    }
    for (&alloc_id, &r) in &info.alloc_region {
        region_anchors.entry(r).or_default().push(ord(alloc_id));
    }
    // Only regions whose `decref_point` lands inside the branch's arms can move,
    // so index the regions by post-order `decref_point` once and let each branch
    // binary-search the arm-interval slices. A move writes the region's new ord
    // straight into the index, which leaves it unsorted, so a branch that moved
    // anything re-sorts before the next one runs — later branches, processed
    // innermost-outward, then search an ordered index and see the post-move values.
    let mut by_dord: Vec<(u32, Region)> = info
        .region_data
        .iter()
        .map(|(&r, d)| (ord(d.decref_point), r))
        .collect();
    by_dord.sort_unstable_by_key(|&(o, _)| o);

    // ── The admission: the frame must be the region's only holder ────────────
    //
    // The anchor is a PLACEMENT argument — one release, moved later — and
    // placement alone is enough only where this frame holds the region's only
    // reference. On the arms the window newly covers the release fires where none
    // did before, so any *other* holder it drops to zero is an over-free; and the
    // reachable other holder is an uncounted borrow in a frame that is PARKED when
    // the release runs, which the resume's uncounted-borrow check detonates on
    // (region/generations.md). No premise about arm structure discharges that.
    //
    // Escape answers exactly this question, and it is the sole authority for it: a
    // value that does not leave its activation by any facet but RETURN is reachable
    // only through this frame's slots for as long as the frame lives, so the frame
    // is the only holder at the merge. Every holder must clear that reading (an
    // aliased region is only as local as its loosest holder), the region must have
    // one (an unheld region offers nothing to judge), and the fiber frontier's
    // atomless site half is refused too, since no binding names it.
    // One predicate, shared with the lowerer's frame-exit release, and computed once
    // by `analyze_regions_with` before any of these passes run — both mechanisms make
    // a release fire where none fired before, so both owe escape the same count
    // argument, and reading it from `RegionInfo` is what keeps them one answer rather
    // than two. A MUTATED route is refused for the reason
    // `region::infer::compensate` refuses it as a release route — a slot repointed
    // between the arm and the anchor frees whatever it holds THEN — and that is asked
    // of the region's own route binding, not of every binding that names the value.
    // Lexical capture is NOT refused: a closure's hold on what it captures is the
    // funnel's counted (or the forest's owning) edge, never the uncounted borrow this
    // admission guards against, and capture by a closure that *escapes* is already
    // one of escape's facets.
    //
    // The **return** facet is no refusal at either placement. At the merge the
    // release follows whatever mint the taken arm already ran (§ "The return
    // frontier is per-path"); at an arm's replica, which does precede its callee's
    // mint, the callee reaches a value this frame owns as an operand or through its
    // captured environment and by no other route — so it either holds a counted edge
    // across the gap or cannot mint against the region at all (mechanism.md § "The
    // callee's return mint, and why the point owes it nothing").
    let frame_held = info.frame_held_regions.clone();

    // Snapshotted for the frame-exit narrowing below, which reads it while
    // `region_data` is borrowed mutably (`RegionInfo::value_routed_regions`).
    let value_routed = info.value_routed_regions.clone();

    // Regions whose release belongs to another mechanism: moving their
    // `decref_point` would move a release that mechanism, not this one, emits.
    let excluded: rustc_hash::FxHashSet<Region> = info
        .region_data
        .keys()
        .copied()
        .filter(|&r| {
            info.suppressed_decref_regions.contains(&r)
                || info.owned_group_members.contains(&r)
                || info.cell_release_regions.contains(&r)
                || info.mutated_binding_value_regions.contains(&r)
                || info.merged_root(r) != r
                || !frame_held.contains(&r)
        })
        .collect();

    for br in &scopes.branches {
        // A nested lambda's own frame exits belong to that lambda, not here.
        let inner_lambdas: Vec<(u32, u32)> = scopes
            .lambdas
            .iter()
            .copied()
            .filter(|&(lo, hi)| br.node_lo <= lo && hi < br.node_hi)
            .collect();
        // The arms that leave through a callee instead of arriving at the merge.
        // The anchor still covers the arms that fall through, and the frame-exit
        // relocation covers the rest — but only for a release it can REPLICATE,
        // which is the value route alone. So their presence narrows the window to
        // the regions a value route can name here rather than declining the branch
        // whole; every other region keeps its in-arm release and compensation's
        // counted routes. `value_routed` asks the question the emitter asks
        // (`lir::lower::regiondecref::value_release_slot`) rather than reading the
        // region's class: releasing by id is the lowerer's DEFAULT, taken wherever a
        // single point covers every path, so a region a binder recorded a slot for
        // takes the value route as soon as a point admits it (mechanism.md § "A
        // release the relocation replicates names a VALUE, and a binder's slot
        // supplies that name").
        let arm_exits: Vec<&FrameExit> = scopes
            .frame_exits
            .iter()
            .filter(|e| {
                e.at >= br.node_lo
                    && e.at <= br.node_hi
                    && !inner_lambdas
                        .iter()
                        .any(|&(lo, hi)| lo <= e.at && e.at <= hi)
            })
            .collect();
        let arm_exits_frame = !arm_exits.is_empty();
        let anchor = last_use.get(&br.id).copied().unwrap_or(br.id);
        let anchor_ord = ord(anchor);
        // Only the barriers nested INSIDE this branch matter: one enclosing the
        // branch encloses the anchor too, so it constrains nothing here.
        let inner_barriers: Vec<(u32, u32)> = scopes
            .barriers
            .iter()
            .copied()
            .filter(|&(lo, hi)| br.node_lo <= lo && hi < br.node_hi)
            .collect();
        // The exemption below only asks whether r is in ANY exiting arm's callee
        // set, so the per-arm sets are merged once per branch and the per-region
        // test is an O(1) membership lookup in that union.
        let callee_union: rustc_hash::FxHashSet<Region> = arm_exits
            .iter()
            .flat_map(|e| e.callee.iter().copied())
            .collect();
        // Merge the half-open barrier intervals into one ordered disjoint list,
        // then answer `any(lo <= dord < hi)` with a binary search. Disjointness is
        // what the search rests on: among disjoint intervals only the LAST whose
        // `lo <= dord` can hold dord, because every earlier one ends at or before
        // that `lo`. Overlapping intervals merge; adjacent ones ([1,5) and [5,8))
        // are left alone, since they are already disjoint — 5 is in the second.
        let mut sorted_barriers = inner_barriers;
        sorted_barriers.sort_unstable_by_key(|&(lo, _)| lo);
        let mut merged_barriers: Vec<(u32, u32)> = Vec::new();
        for &(lo, hi) in &sorted_barriers {
            if let Some(last) = merged_barriers.last_mut() {
                if lo < last.1 {
                    if hi > last.1 {
                        last.1 = hi;
                    }
                    continue;
                }
            }
            merged_barriers.push((lo, hi));
        }
        let in_barrier = |dord: u32| -> bool {
            let i = merged_barriers.partition_point(|&(lo, _)| lo <= dord);
            i > 0 && merged_barriers[i - 1].1 > dord
        };
        // Merge the arms' closed intervals (`lo <= dord <= hi`) so each region in
        // the branch's union is scanned once, then binary-search the `by_dord`
        // index for each merged interval.
        let mut arm_ivs: Vec<(u32, u32)> = br.arms.iter().map(|a| (a.lo, a.hi)).collect();
        arm_ivs.sort_unstable_by_key(|&(lo, _)| lo);
        let mut merged_arms: Vec<(u32, u32)> = Vec::new();
        for (lo, hi) in arm_ivs {
            if let Some(last) = merged_arms.last_mut() {
                if lo <= last.1 {
                    if hi > last.1 {
                        last.1 = hi;
                    }
                    continue;
                }
            }
            merged_arms.push((lo, hi));
        }
        // Snapshot the branch's candidates before any move: the inner loop mutates
        // `by_dord` in place (a move raises the entry's ord), so iterating the live
        // slice would shift not-yet-visited entries across the fixed `end` boundary
        // and admit regions whose ord falls OUTSIDE the arm union. Collecting the
        // union's entries once — each region appears in exactly one merged interval —
        // matches the original per-region pass, which read every `decref_point`
        // before mutating any of them.
        let mut candidates: Vec<(u32, Region, usize)> = Vec::new();
        for &(lo, hi) in &merged_arms {
            let start = by_dord.partition_point(|&(o, _)| o < lo);
            let end = by_dord.partition_point(|&(o, _)| o <= hi);
            for (rel, &(o, r)) in by_dord[start..end].iter().enumerate() {
                candidates.push((o, r, start + rel));
            }
        }
        let mut moved = false;
        for &(dord, r, idx) in &candidates {
            if excluded.contains(&r) {
                continue;
            }
            if arm_exits_frame && !value_routed.contains(&r) {
                continue;
            }
            // A region the exiting arm's call names as its CALLEE is exempt from the
            // relocation, so no replica reaches that arm — and what stands in for the
            // release there is the deferred callee channel, which is keyed on where
            // the release SITS (`deferred_release_slot`; mechanism.md § "What the
            // exemption keeps, a channel must still run"). Moving it to the merge
            // takes it out of that channel's reach and leaves the arm with nothing,
            // so such a region keeps its in-arm release. An ARGUMENT's exemption is
            // the opposite story — the release the arm never runs IS the ownership
            // move, and the callee's owned-parameter release consumes it — so an
            // argument is no refusal here (rules.md Rule 5).
            if callee_union.contains(&r) {
                continue;
            }
            if dord >= anchor_ord {
                continue;
            }
            // A boundary is the scope's BODY, not the scope's own node: the
            // lowerer emits a node's releases after it finishes lowering that
            // node, so a `decref_point` AT the `While`/`Loop` lands after the loop
            // and runs once per execution of it — the count the merge label is
            // reached with. A `Lambda` node reads the same way; only its body runs
            // in another activation. The interval is therefore half-open on the
            // high end, which is what admits a live-in region a nested loop merely
            // READS: the loop-node extension anchors every such read at the loop
            // node (`hir/liveness/lastuse`), so the closed reading would leave the
            // branch's only release under the looping arm.
            if in_barrier(dord) {
                continue;
            }
            let live_in = region_anchors
                .get(&r)
                .is_some_and(|a| a.iter().all(|&o| o < br.node_lo || o > br.node_hi));
            if !live_in {
                continue;
            }
            // The window moves only where the release is EMITTED. `lifetime_point`
            // stays at the structural last use, so the ownership and merge cuts —
            // whose post-dominance obligations are lifetime questions — keep
            // reading the region's real lifetime and not this anchor
            // (`RegionData::lifetime_point`).
            info.region_data.get_mut(&r).unwrap().decref_point = anchor;
            by_dord[idx].0 = anchor_ord;
            moved = true;
        }
        // A move raises a region's `decref_point` in place and so unsorts the
        // index; restore the order so the next branch's interval searches hold.
        // Most branches move nothing, and those owe no sort at all.
        if moved {
            by_dord.sort_unstable_by_key(|&(o, _)| o);
        }
    }
}
