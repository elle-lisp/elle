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
//! funds it, so the per-arm decref provably cannot drop a live value to zero (an env
//! cell's box needs no such retain — see below). Three such retains:
//!  - the **stored value** — a `put`/`push`/`add` funnel raises the value's RC, so
//!    releasing the wrapper's own reference to it leaves the container's reference
//!    live (`funnel_store_sites`);
//!  - the **`-mut` container** — a mutable-container funnel returns arg0 in place,
//!    so the wrapper's mutable arm hands the container back pass-through and never
//!    releases the owned-param reference it holds. The funnel's `pass_through_retain`
//!    raised the RETURNED value's RC, so releasing that stranded owned-param
//!    reference leaves the live returned container ≥ 1 (`funnel_container_sites`).
//!    Freeing the container promptly cascades its stored heap members through the
//!    outgoing-edge table, so one per-arm release reclaims the whole subtree.
//!  - the **return mint** — at a `Return` node `lower_return` raises the returned
//!    region's RC for the caller, before the node's own releases, so a per-arm decref
//!    keyed there drops the callee's reference and leaves the caller's. This is the
//!    base case of a walk (`(if (= i 0) xs (go … xs))`), whose returning arm loses
//!    the `decref_point` max to the recursive arm's later use and is otherwise left
//!    with a mint and no release at all.
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
//! compensating would double-free: merge children (the root's single decref frees
//! them), co-owned-group members, mutated-slot 1-slot containers, and the
//! already-`suppressed_decref_regions`.
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
//! yielding the box (docs/impl/region/mechanism.md § "A compensating release of an
//! env cell names the box, not the holder's slot"). What the `tail` route still owes
//! is placement, so the box's per-arm release is read off the same pins the global
//! `decref_point` is: the arm's uses, AND the arm's uncounted opcode-read borrows
//! out of the cell, whose reader the release must post-date.
//!
//! A compiled FORWARD cell is a different region and is refused by both routes — its
//! holder binding is captured by construction, so the capture taint reaches it, and
//! its release is a `DecrefRegion` by static slot rather than a box load.
//!
//! The **return frontier** is an exclusion of a different kind, and it is
//! *per-path*: a returned region is the caller's to free only on the paths that
//! actually hand it over (docs/impl/region/mechanism.md § "The return frontier is
//! per-path"). So a return-escaping region is admitted here on exactly the arms
//! where no hand-over happened, or where the hand-over's own mint funds the release:
//!  - **head**, unconditionally — the arm has no use of the region at all, so no mint
//!    fires on this path, the caller receives nothing, and the callee's reference is
//!    the only one in existence. Otherwise the whole region — and every member its
//!    free cascade would reclaim — is held to fiber teardown;
//!  - **tail**, through the return-mint guard above, which is the hand-over site
//!    itself. Any other `tail` guard keeps the frontier exclusion: a store site's
//!    retain says nothing about whether this arm also returns the value.
//!  - the `-mut` funnel **container** is the one non-return retain admitted past the
//!    frontier: it is handed back pass-through yet holds a *distinct* stranded
//!    owned-param reference no return covers (`mut_container_regions`).
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
//! The `tail` route's same-node retain requirement is **not** a redundant belt on
//! the placement argument, and must not be relaxed into "any arm-last-use node".
//! Placement symmetry says only that the release lands after this arm's last *named*
//! use; it does not say the callee's reference is the only one in existence there.
//! An arm that USES the region can have handed out a reference the solver does not
//! name — an uncounted borrow held by a suspended frame's activation region map is
//! the reachable one — and a release that reaches zero then frees a region a parked
//! fiber still resolves through its slot (the generation stamp detonates it at the
//! resume, `docs/impl/region/generations.md` § "Uncounted-borrow check"). The retain
//! on the node is what makes the per-arm decref provably non-zeroing, which is why
//! a used sibling arm with no such retain keeps the baseline. The dead sibling arm
//! needs no retain precisely because it creates no reference at all, and the env
//! cell below needs none because its box's holders are known outright.
//!
//! This pass sees only the regions the **branch-arm release window** declined
//! (`analyze/decref.rs`, docs/impl/region/mechanism.md § "A release inside one arm
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
/// iteration reads a recycled cell (docs/impl/region/bindings.md § "Env cells in
/// loops: release once per activation, not per iteration").
///
/// The loop must lie INSIDE the arm — a loop enclosing the whole branch is not a
/// point this arm can host, and the loop-invariant guard has already refused the
/// region if one encloses the branch but not the cell's allocation. `None` when
/// `at` is in no such loop, or is already at or past the loop's own node.
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
    //
    // Both taints are claims about a release routed through the holder's SLOT, so
    // neither reaches an env cell, whose release names the cell BOX instead — a box
    // `populate_env` mints once per activation, that an `assign` never repoints (it
    // writes the cell's content), and that a capturer holds by the funnel's counted
    // `closure ⊇ cell` edge rather than through this frame's slot. Read per region,
    // exactly as the frame-exit admission reads the same two facts (module doc;
    // docs/impl/region/mechanism.md § "A compensating release of an env cell names
    // the box, not the holder's slot").
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
            if unsafe_holder && !info.cell_release_regions.contains(&r) {
                tainted.insert(r);
            }
        }
    }
    for (&alloc_id, &r) in &info.alloc_region {
        region_anchors.entry(r).or_default().push(alloc_id);
    }
    // Region → the uncounted opcode reads (`%get`/`%first`/`%rest`) that borrow out
    // of it. Such a read hands back a value living inside the container and raises
    // no count on it, so the container's release must post-date the READER, not the
    // read (`analyze/decref.rs`, `uncounted_read_sites`). The global `decref_point`
    // already carries that extension; the env-cell `tail` route below carries it per
    // arm, since that route has no same-node retain to prove the borrow counted.
    let mut region_reads: HashMap<Region, Vec<HirId>> = HashMap::new();
    for (&read_id, containers) in &info.uncounted_read_sites {
        for &r in containers {
            region_reads.entry(r).or_default().push(read_id);
        }
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

    // The CONTAINER compensation input: a `-mut` retaining store returns its
    // container (arg0) pass-through, so a dispatch wrapper's mutable arm hands the
    // container back and never releases the owned-param reference it holds — a leak
    // on the container's own region (and, by the outgoing-edge cascade, every heap
    // member stored into it). The funnel's `pass_through_retain` raised the RETURNED
    // value's RC, so a per-arm decref of the container here releases only that
    // stranded owned-param reference and can never drop the live returned container
    // to zero — the exact dual of the stored-value guard below. Because the container
    // is return-escaping, the ordinary return-frontier exclusion would suppress this
    // compensation, so the container is admitted through that gate
    // (`mut_container_regions`) while every other exclusion still applies.
    let mut container_at_site: HashMap<HirId, std::collections::HashSet<Region>> = HashMap::new();
    let mut mut_container_regions: std::collections::HashSet<Region> =
        std::collections::HashSet::new();
    for (&site, containers) in &info.funnel_container_sites {
        container_at_site
            .entry(site)
            .or_default()
            .extend(containers.iter().copied());
        mut_container_regions.extend(containers.iter().copied());
    }

    let excluded = |r: Region| -> bool {
        info.suppressed_decref_regions.contains(&r)
            || info.owned_group_members.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || info.merged_root(r) != r
            || tainted.contains(&r)
    };

    // The return-frontier gate, consulted by `tail_admitted` below and by nothing
    // else — `head` compensation is admitted unconditionally, since an arm with no
    // use of `r` cannot have handed it to a caller (module doc; mechanism.md § "The
    // return frontier is per-path"). The `-mut` funnel container is exempt: it is
    // handed back pass-through yet holds a DISTINCT stranded owned-param reference no
    // return covers.
    let tail_excluded =
        |r: Region| -> bool { tail_regions.contains(&r) && !mut_container_regions.contains(&r) };

    // A `tail` per-arm decref is sound only at a node whose own instructions raise
    // the value's RC on the same path — for a STORE, the stored value of
    // `put`/`set`/`push`. The store raises the value's RC (compile-time `IncrefRegion`,
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
    // The byte-copy dual: a `%string-push`/`%bytes-push` funnel copies the value's
    // bytes and touches neither its incref nor its decref, so the wrapper's stranded
    // `val` per-arm release is the value's TRUE last use — sound at the same tail
    // sites as a retaining store's (and `%del`'s in-body decref is excluded upstream,
    // so no double-free reaches here).
    for (&site, vals) in &info.funnel_bytecopy_value_sites {
        store_value_at_site
            .entry(site)
            .or_default()
            .extend(vals.iter().copied());
    }
    // The return dual, and the one guard that admits a RETURN-ESCAPING region to the
    // `tail` route: at a `Return` node `lower_return` mints the caller's owning
    // reference to the returned value's region, and the lowerer emits that mint
    // BEFORE the node's releases. So a per-arm decref keyed here releases the
    // callee's own reference and leaves the caller's — the same "the retain on this
    // node funds this release" argument the stores above make. This is the base case
    // of a walk (`(if (= i 0) xs (go … xs))`): the recursive arm's later use wins the
    // `decref_point` max, so without this the returning arm carries a mint and no
    // release at all (docs/impl/region/mechanism.md § "The return frontier is
    // per-path").
    let mut return_mint_at_site: HashMap<HirId, std::collections::HashSet<Region>> = HashMap::new();
    for (site, regions) in return_sites {
        return_mint_at_site
            .entry(*site)
            .or_default()
            .extend(regions.iter().copied());
    }

    // Is a `tail` release of `r` at `node` funded by a retain on that same node?
    // The return mint doubles as the per-path return-frontier admission — it IS the
    // hand-over — while the store and container retains say nothing about whether
    // this arm also returns the value, so they keep the frontier exclusion.
    //
    // The container case: `node` is a monomorphic funnel whose container (arg0) is
    // `r`. A `-mut` funnel returns the container pass-through (its
    // `pass_through_retain` keeps the returned value's RC ≥ 1 after the per-arm
    // release of the stranded owned-param reference); an immutable funnel returns a
    // fresh copy, so the container is genuinely dead in the arm.
    let tail_admitted = |r: Region, node: HirId| -> bool {
        if return_mint_at_site
            .get(&node)
            .is_some_and(|s| s.contains(&r))
        {
            return true;
        }
        if tail_excluded(r) {
            return false;
        }
        store_value_at_site
            .get(&node)
            .is_some_and(|s| s.contains(&r))
            || container_at_site.get(&node).is_some_and(|s| s.contains(&r))
    };

    let mut head: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut tail: HashMap<HirId, Vec<Region>> = HashMap::new();
    let mut container_release_sites: std::collections::HashSet<HirId> =
        std::collections::HashSet::new();
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
            let crosses_loop = loops.iter().any(|l| {
                let c_in = l.lo <= br.node_lo && br.node_hi <= l.hi;
                c_in && anchors.iter().any(|&a| ord(a) < l.lo || ord(a) > l.hi)
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
                            region_reads
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
                    } else if holder_count.get(&r).copied().unwrap_or(0) == 1
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
                            && holder_count.get(&r).copied().unwrap_or(0) == 1
                            && tail_admitted(r, node) =>
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
                        if container_at_site.get(&node).is_some_and(|s| s.contains(&r))
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
