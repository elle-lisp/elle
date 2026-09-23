// audited: 2026-09-23
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
//! A `tail` release of a VALUE is admitted only where a retain on the SAME node
//! funds it, so the per-arm decref provably cannot drop a live value to zero; an
//! env cell's box needs no such retain. Which retains count is [`admit`], and
//! what each region's premises are is [`premises`].
//!
//! Soundness rests on structural facts about region `r` and a branch `C` one of
//! whose arms holds `r`'s `decref_point`:
//!  - **the leak is real and in-arm**: `r`'s `decref_point` is inside an arm of
//!    `C`, so its last use is within `C` (nothing uses `r` after `C`);
//!  - **live-in**: every anchor of `r` (its `alloc_region` HirIds and its route
//!    binder's def site) is OUTSIDE `C`'s subtree, so `r` is allocated before `C`
//!    and enters every arm live with RC ≥ 1;
//!  - **loop-invariant guard**: no `While`/`Loop` encloses `C` but not `r`'s
//!    allocation — otherwise `r` is allocated once outside the loop and a
//!    per-iteration arm free would reclaim it mid-loop (a use-after-free, the same
//!    hazard the `Var` iter-scope extension in `liveness/lastuse.rs` guards). When
//!    the alloc IS inside the loop with `C`, the per-iteration free is correct.
//!
//! An **env cell** — a `cell_release_regions` member — takes BOTH routes, and every
//! refusal that would decline one is a claim about the *value* its holder binding
//! names rather than about the box. Its release names the cell BOX, through
//! `LoadCaptureRaw` + `DecrefCellRegion`: a box `populate_env` mints once per
//! activation, which an `assign` never repoints (it writes the content), and which a
//! capturer reaches through the counted `closure ⊇ cell` edge the funnel took. So
//! the two slot-route taints — a MUTATED holder, a CAPTURED holder — miss it, and so
//! do the `tail` route's own two gates: the RETURN frontier withholds a region the
//! caller now holds a reference to, and what a return hands over is the content;
//! the same-node RETAIN buys the knowledge that the arm's use named every
//! reference, which the box's holders supply outright, no use of the binding ever
//! yielding the box (docs/impl/region/compensate.md § "A compensating release of an
//! env cell names the box, not the holder's slot"). What the `tail` route still owes
//! is placement, so the box's per-arm release is read off the same pins the global
//! `decref_point` is: the arm's uses, AND the arm's uncounted opcode-read borrows
//! out of the cell, whose reader the release must post-date.
//!
//! A compiled FORWARD cell is a different region and is refused by both routes — its
//! holder binding is captured by construction, so the capture taint reaches it, and
//! its release is a `DecrefRegion` by static slot rather than a box load.
//!
//! Both routes read the **arm structure**, never the branch's arity or kind: an
//! `If`'s two arms and a `Match`'s N arms are alike mutually exclusive, and at most
//! one body runs per execution. Every premise above is stated over one arm and its
//! siblings, so a `Match` arm is admitted by the identical argument (a `Match` with
//! no matching arm runs no body at all — the compensation simply does not fire,
//! which is the leak-preserving direction). What counts as an arm is
//! [`super::arms`]: the short-circuiting forms (`cond`, `and`, `or`) contribute
//! the arms of their nested-`If` equivalent, one branch per conditional level, so
//! a clause test is an arm exactly as a clause body is. The levels of one form
//! partition the paths — the compensating side of level *k* is disjoint from every
//! other level's — so a path takes one compensating release or the region's own
//! `decref_point`, never both.
//!
//! This pass sees only the regions the **branch-arm release window** declined
//! (`analyze/decref.rs`, docs/impl/region/window.md § "A release inside one arm
//! is not a release on the other arms"). Where that window applies it moves the
//! region's single `decref_point` out of the arms entirely, so `arm_of_d` below
//! finds nothing and neither route fires — one anchored release replaces the
//! per-arm ones. The two partition the obligation: the window answers "does this
//! frame hold the region alone" with escape and needs no count argument;
//! everything escape cannot clear arrives here, where the count argument is the
//! retain.

use super::arms::ArmSet;
use super::*;
use crate::hir::region::Region;

// What the arm loop below reads, each its own subject: the per-region premises
// it tests, and what funds a `tail` release at the node it would be keyed to.
mod admit;
mod premises;

use admit::TailFunding;
use premises::Premises;

/// The two per-arm compensation maps. `head[arm_body]` releases at that arm's head
/// (dead sibling arm); `tail[node]` releases after that node — the region's last
/// use within a used sibling arm — through `emit_decrefs_for`.
pub(super) struct BranchComp {
    pub head: HashMap<HirId, Vec<Region>>,
    pub tail: HashMap<HirId, Vec<Region>>,
    /// Funnel sites where a CONTAINER (not a stored value) was released by the tail
    /// compensation — the lowerer drops the redundant tail ReturnValue retain here.
    /// See `RegionInfo::container_release_sites`.
    pub container_release_sites: std::collections::HashSet<HirId>,
}

/// A `While`/`Loop`'s node and post-order subtree interval, for the
/// loop-invariant guard and for the env cell's once-per-activation hoist.
struct IterScope {
    id: HirId,
    lo: u32,
    hi: u32,
}

/// The node an env cell's per-arm release takes instead of `at`, so that it
/// fires once per execution of the arm rather than once per iteration of a loop
/// inside it: the outermost `While`/`Loop` that encloses `at` and is itself
/// contained in the arm, which the lowerer emits after the loop.
///
/// This is the per-arm analogue of the global `decref_point`'s
/// `post_loop_placement` (`analyze/decref.rs`), and it exists for the same
/// reason: `populate_env` mints the box once per activation, so a release
/// anchored at an in-loop use frees it on the first iteration and the next
/// iteration reads a recycled cell (docs/impl/region/cells.md § "Env cells in
/// loops: release once per activation, not per iteration").
///
/// The loop must lie INSIDE the arm — a loop enclosing the whole branch is not a
/// point this arm can host, and no such loop reaches here. One that encloses the
/// branch but not the cell's allocation is refused by the loop-invariant guard.
/// One that encloses both is kept out by the env-cell loop hoist
/// (`post_loop_placement`, `analyze/decref.rs`): the cell's global `decref_point`
/// already sits at the outermost enclosing loop, outside every arm of a branch
/// inside it, so `arm_of_d` finds nothing. `None` when `at` is in no such loop,
/// or is already at or past the loop's own node.
fn arm_post_loop_placement(
    at: HirId,
    loops: &[IterScope],
    arm_lo: u32,
    arm_hi: u32,
    order: &HashMap<HirId, u32>,
) -> Option<HirId> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let at_ord = ord(at);
    loops
        .iter()
        .filter(|l| l.lo <= at_ord && at_ord <= l.hi && arm_lo <= l.lo && l.hi <= arm_hi)
        .max_by_key(|l| l.hi)
        .map(|l| l.id)
        .filter(|&id| ord(id) > at_ord)
}

/// Compute the per-arm compensating decrefs (`head` + `tail`). See the module doc
/// for the soundness conditions. `last_use` maps each use HirId to its consuming
/// (decref-safe) node — the same `compute_last_use` result `decref_point` is
/// computed from, so a `tail` release placed at an arm's max last-use node is the
/// per-arm analogue of the global `decref_point` and is decref-safe by symmetry.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_branch_compensation(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    du: &DefUseBuilder,
    arena: &BindingArena,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
    return_sites: &[(HirId, Vec<Region>)],
    binder_init_sites: &HashMap<crate::hir::binding::Binding, Option<HirId>>,
) -> BranchComp {
    // The per-binding source regions are the solver's `binding_source_regions`
    // (== `inference_binding_regions`, already mirrored onto `info` before this
    // runs), so read them from `info` rather than threading a redundant param.
    let binding_regions = &info.binding_source_regions;
    let low = compute_subtree_low(hir, order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    let mut branches: Vec<ArmSet> = Vec::new();
    let mut loops: Vec<IterScope> = Vec::new();
    collect(hir, order, &low, &mut branches, &mut loops);
    if branches.is_empty() {
        return BranchComp {
            head: HashMap::new(),
            tail: HashMap::new(),
            container_release_sites: std::collections::HashSet::new(),
        };
    }

    let p = Premises::collect(hir, info, du, arena, binding_regions, binder_init_sites);
    let funding = TailFunding::collect(info, escape, return_sites);

    let mut head: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut tail: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut container_release_sites: std::collections::HashSet<HirId> =
        std::collections::HashSet::new();
    for (&r, uses) in &p.uses {
        if uses.is_empty() || p.excluded(info, r) {
            continue;
        }
        let Some(d) = info.region_data.get(&r).map(|rd| ord(rd.decref_point)) else {
            continue;
        };
        let anchors = match p.anchors.get(&r) {
            Some(a) => a,
            None => continue,
        };
        for br in &branches {
            // Which arm holds the decref_point? (The leak is real only when the
            // release lands inside an arm — an extension that hoisted it out, or a
            // post-dominating point, leaves `d` outside every arm here.)
            let arm_of_d = br.arms.iter().position(|a| d >= a.lo && d <= a.hi);
            let Some(di) = arm_of_d else { continue };
            // live-in: every anchor is outside C's subtree.
            let live_in = anchors
                .iter()
                .all(|&a| ord(a) < br.node_lo || ord(a) > br.node_hi);
            if !live_in {
                continue;
            }
            // loop-invariant guard: no loop encloses C but not r's allocation.
            // Asked of the ALLOCATION sites where the region has any — a holder
            // defined outside the loop (a cell the loop stores into) says
            // nothing about where the value is born, and the per-iteration
            // release is correct exactly when every birth is per-iteration. A
            // region with no recorded allocation (an env cell's phantom, a
            // parameter's content) falls back to the anchor set, which is then
            // the only birth reading there is.
            let allocs = p.allocs.get(&r).unwrap_or(anchors);
            let crosses_loop = loops.iter().any(|l| {
                let c_in = l.lo <= br.node_lo && br.node_hi <= l.hi;
                c_in && allocs.iter().any(|&s| ord(s) < l.lo || ord(s) > l.hi)
            });
            if crosses_loop {
                continue;
            }
            for (ai, a) in br.arms.iter().enumerate() {
                let (arm_id, arm_lo, arm_hi) = (a.id, a.lo, a.hi);
                if ai == di {
                    continue; // the arm whose in-arm decref is the global decref_point
                }
                // An env cell routes by its own reading of the same question, since
                // its `tail` release carries no same-node retain to stand in for the
                // placement (module doc). The candidates are what the global
                // `decref_point` is a max over, restricted to this arm: each in-arm
                // use's consuming node, plus the reader of each in-arm uncounted
                // opcode read that borrows out of the cell. A candidate landing
                // OUTSIDE the arm is not a point this arm can host — ANF may float a
                // consumer past its own arm — and the arm is then declined by both
                // routes rather than approximated: the `head` release would precede
                // the very use that candidate came from.
                if info.cell_release_regions.contains(&r) {
                    let in_arm = |id: HirId| {
                        let o = ord(id);
                        o >= arm_lo && o <= arm_hi
                    };
                    let candidates: Vec<HirId> = uses
                        .iter()
                        .copied()
                        .filter(|&u| in_arm(u))
                        .chain(
                            p.reads
                                .get(&r)
                                .into_iter()
                                .flatten()
                                .copied()
                                .filter(|&rd| in_arm(rd)),
                        )
                        .map(|n| last_use.get(&n).copied().unwrap_or(n))
                        .collect();
                    if candidates.is_empty() {
                        // Dead sibling arm: the head release, as below.
                        head.entry(arm_id).or_default().push(r);
                    } else if p.single_holder(r)
                        && candidates.iter().all(|&n| in_arm(n) && ord(n) != d)
                    {
                        let node = candidates
                            .into_iter()
                            .max_by_key(|&n| ord(n))
                            .expect("a non-empty candidate set has a max");
                        // Once per execution of the arm, never once per
                        // iteration of a loop inside it.
                        let node = arm_post_loop_placement(node, &loops, arm_lo, arm_hi, order)
                            .unwrap_or(node);
                        tail.entry(node).or_default().push(r);
                    }
                    continue;
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
                    //
                    // An env cell never reaches here — it routed above, off the box's
                    // own holders rather than off a retain (module doc).
                    Some(node)
                        if info.call_result_regions.contains(&r)
                            && p.single_holder(r)
                            && funding.admits(r, node) =>
                    {
                        // Record the site for the lowerer's ReturnValue suppression
                        // ONLY when the funnel is a `-mut` PASS-THROUGH (the result IS
                        // this container, whose reference the caller already owns). An
                        // immutable funnel's container is compensated above but its FRESH
                        // result keeps its ReturnValue retain — the caller's move/reassign
                        // reference, whose suppression would over-free a result stored
                        // into a reassigned slot. Pinned by `set-add`/`struct-put`/
                        // `del-wrapper`/`native-tail-put-*` and the container-compensation
                        // guardfree fixture.
                        if funding.container_at(r, node)
                            && info
                                .funnel_passthrough_sites
                                .get(&node)
                                .is_some_and(|s| s.contains(&r))
                        {
                            container_release_sites.insert(node);
                        }
                        tail.entry(node).or_default().push(r)
                    }
                    Some(_) => {}
                    // Dead sibling arm (no use at all): head release. Admitted on
                    // every arm of every branch kind — the arm creates no reference
                    // to `r`, so the callee's own is the only one in existence here.
                    None => head.entry(arm_id).or_default().push(r),
                }
            }
        }
    }
    // Deterministic per-node order (region id), independent of hash iteration.
    for regions in head.values_mut().chain(tail.values_mut()) {
        regions.sort_by_key(|r| r.0);
        regions.dedup();
    }
    BranchComp {
        head,
        tail,
        container_release_sites,
    }
}

/// Collect every branch (with its arms' intervals — [`super::arms`]) and every
/// `While`/`Loop` (with its subtree interval) in one walk.
fn collect(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    branches: &mut Vec<ArmSet>,
    loops: &mut Vec<IterScope>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = |id: HirId| low.get(&id).copied().unwrap_or(0);
    if let HirKind::While { .. } | HirKind::Loop { .. } = &hir.kind {
        loops.push(IterScope {
            id: hir.id,
            lo: lo(hir.id),
            hi: ord(hir.id),
        });
    }
    branches.extend(super::arms::branch_arms(hir, order, low));
    hir.for_each_child(|c| collect(c, order, low, branches, loops));
}
