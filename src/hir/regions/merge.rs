//! The builder-idiom merge seed (`super` = `hir::regions`).
//!
//! Computes `RegionInfo::merged_parent`: the `child → parent` forest of regions
//! the lowerer may collapse onto one physical region. The seed is narrow and
//! purpose-built — a freshly-built child aggregate merged into the parent `%pair`
//! it is stored into — and its soundness is pinned entirely by the predicate
//! below (docs/impl/region/merging.md § Merging). The lowerer consumes it through
//! `static_slot`'s `merged_root` canonicalization: every member of a merge tree
//! resolves to the root's slot, so child and parent allocate into one physical
//! region, the child's `DecrefRegion` is suppressed, and the merged store edge's
//! `IncrefRegion` is dropped. With no merge (`merged_parent` empty) `merged_root`
//! is the identity and the lowerer's behaviour is the unmerged baseline.
//!
//! Why it is safe to merge across the `child → parent` store edge — the very edge
//! an "exclude any edge-connected region" first cut would forbid — is that the
//! gates pin the child neither outlives the parent (its `decref_point` is at or
//! before the parent's) nor is held anywhere else (sole-held, non-escaping, stored
//! solely into this parent), with the parent itself a non-escaping local owner.
//! The merged `child → parent` edge becomes an intra-region self-edge, which the
//! free-time cascade already skips, so the structure frees as a unit.

use super::postdom::{EmitMode, PostDom};
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// Compute the builder-idiom merge forest over a fully-built `RegionInfo` (its
/// `region_data` decref_points final — the lifetime gate reads them) and the
/// structural execution `order` (so lifetimes compare by execution position, not
/// `HirId` magnitude, which ANF makes meaningless). Returns `child → parent` for
/// every region a builder-idiom merge collapses.
pub(super) fn compute_merges(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    order: &HashMap<HirId, u32>,
) -> HashMap<Region, Region> {
    // The `%pair` intrinsic nodes — the canonical immutable aggregate-store sites.
    // (Together with `alloc_region[site] == dst` below, this uniquely selects a
    // pair's own car/cdr store edges out of `cross_region_refs`; push/put/set-cell
    // and clique edges target something other than the site's own allocation.)
    let mut pair_sites: FxHashSet<HirId> = FxHashSet::default();
    collect_pair_sites(hir, &mut pair_sites);
    if pair_sites.is_empty() {
        return HashMap::new();
    }

    // Per-source store targets: which regions each source region is stored into.
    // Builds the sole-target check (a child stored into exactly one parent) in
    // O(1) per edge instead of rescanning `cross_region_refs`.
    let mut src_targets: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
    for &(_, src, dst) in &info.cross_region_refs {
        src_targets.entry(src).or_default().insert(dst);
    }

    // Regions that flow to a tail/return (the return frontier) — a returned child
    // outlives its parent's free. Projected from escape's authoritative return
    // verdict (`super::escape`), not a solver-local tail set.
    let returned_regions: FxHashSet<Region> = super::escape::return_frontier_regions(
        escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );

    // Region → distinct user holders, via the shared index (`regions::holders`).
    // Synthetic ANF producer temps are excluded by the type, so the common builder
    // case — an inner pair bound only to an ANF temp — is sole-held by construction;
    // a region held by two USER bindings is an alias and refused (`len() > 1`
    // below). The merge seed admits any non-synthetic holder, so its eligibility
    // predicate is `true` (the read-eligibility filter is the reassign gate's, not
    // the seed's).
    let region_holders = super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );

    // A region is a fresh local immutable aggregate iff it holds a real local
    // allocation and is none of the dynamic classes (call-result placeholder,
    // capture cell, reassign-suppressed, mutated 1-slot-container value). A `%pair`
    // alloc region satisfies this; a runtime-fact region never does.
    let fresh_local = |r: Region| -> bool {
        info.live_regions.contains(&r)
            && !info.call_result_regions.contains(&r)
            && !info.cell_release_regions.contains(&r)
            && !info.suppressed_decref_regions.contains(&r)
            && !info.mutated_binding_value_regions.contains(&r)
    };

    // Refuse the merge when the child's holder either RETURNS or is CAPTURED — two
    // DIFFERENT questions answered by two DIFFERENT authorities, not one "escape" check:
    //
    //  - RETURN is a lifetime question, so it reads escape (the authority):
    //    `binding_escapes_via_return`. We cannot read the full `binding_escapes_activation`
    //    here — storing the child INTO the parent is the builder idiom's own (allowed)
    //    store-escape, which the full set folds in and would wrongly refuse — so the
    //    return facet is the exposed, precise sub-question.
    //
    //  - CAPTURE is a REACHABILITY question, not a lifetime one. The merge's soundness
    //    rests on the child being reachable ONLY through the parent: gates 1+4 make it
    //    sole-STORED, and this clause makes it sole-HELD — a holder that a closure also
    //    captures is a second reachability path the parent's single drop does not own. So
    //    it needs the UNCONDITIONAL "is it captured at all" relation — deliberately NOT
    //    escape's capture FACET, which asks the strictly narrower, CONDITIONAL question
    //    "captured by a closure that itself escapes." A child captured by a non-escaping
    //    closure escapes nothing yet is still doubly held, and the merge must refuse it all
    //    the same. Escape is the authority for "does it outlive its activation"; it is NOT
    //    the authority for "is it uniquely held here" — that is the region forest's own
    //    reachability question, answered by the region capture-graph
    //    (`super::escape::captured_bindings`: the bindings some closure captures, sourced
    //    structurally from the HIR — never the lexical proxy `is_captured` the solver is
    //    locked out of).
    let captured = super::escape::captured_bindings(hir);
    let holder_non_escaping = |child: Region| -> bool {
        match region_holders.holders_of(child) {
            // No user holder (an ANF-temp-only child) — nothing to escape.
            None => true,
            // More than one user binding holds it: aliased — refuse.
            Some(hs) if hs.len() > 1 => false,
            Some(hs) => hs
                .iter()
                .all(|&b| !escape.binding_escapes_via_return(b) && !captured.contains(&b)),
        }
    };

    // Structural post-dominance over the scope tree — gate 6's authority. Built
    // once (only reached past the pair-site early-return), queried per edge.
    let dom = PostDom::new(hir, order);

    let mut merged_parent: HashMap<Region, Region> = HashMap::new();
    for &(site, child, parent) in &info.cross_region_refs {
        // (1) An immutable `%pair` car/cdr store: a Pair node whose edge target is
        //     the aggregate freshly allocated AT that site, and not a clique site.
        if child == parent
            || !pair_sites.contains(&site)
            || info.alloc_region.get(&site) != Some(&parent)
            || info.hard_edge_sites.contains(&site)
        {
            continue;
        }
        // (2) parent and (3) child are both fresh local immutable aggregates.
        if !fresh_local(parent) || !fresh_local(child) {
            continue;
        }
        // (4) The child is stored ONLY into this parent (not aliased across
        //     aggregates). Repeated stores into the SAME parent are fine.
        if src_targets
            .get(&child)
            .is_none_or(|ts| ts.len() != 1 || !ts.contains(&parent))
        {
            continue;
        }
        // (5) Neither child nor parent escapes. A returned/captured CHILD outlives
        //     the parent's free. A returned/captured PARENT is sound to merge too
        //     (the child still dies within it) but is the deferred widening — start
        //     narrow with the parent as a genuine LOCAL owner (the discarded /
        //     together-consumed nested literal), widen cut by cut
        //     (docs/impl/region/merging.md § Merging).
        if returned_regions.contains(&child)
            || returned_regions.contains(&parent)
            || !holder_non_escaping(child)
            || !holder_non_escaping(parent)
        {
            continue;
        }
        // (6) The parent's free POST-DOMINATES the child's last use — the single
        //     drop point (the shared region's `DecrefRegion` at the parent's
        //     `decref_point`) must not precede the child's own last *direct* use.
        //     Decided STRUCTURALLY over the scope tree, not by `ord` magnitude
        //     (region/merging.md § Merging, condition 6). `EmitMode::Merge`
        //     waives the loop-enclosure clause: gates 1+4 make the child reachable
        //     only through the parent (containment), so a loop rebuilding the parent
        //     rebuilds the only path to the child — an in-loop nested literal still
        //     merges, no cross-iteration re-deref. A child READ after the parent's
        //     death (the aliased / mutable-accumulator shape) is sequenced after the
        //     parent's free and is refused.
        let (Some(cd), Some(pd)) = (
            info.region_data.get(&child).map(|d| d.decref_point),
            info.region_data.get(&parent).map(|d| d.decref_point),
        ) else {
            continue;
        };
        if !dom.drop_post_dominates(pd, cd, EmitMode::Merge) {
            continue;
        }
        // The merged shape's numeric shadow: a child that does not outlive its
        // parent does not linearize after it. A debug echo of the structural
        // verdict, never the deciding test.
        #[cfg(debug_assertions)]
        {
            let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
            debug_assert!(
                ord(cd) <= ord(pd),
                "merge admitted child r{} whose decref_point ({}) linearizes after \
                 parent r{}'s ({})",
                child.0,
                ord(cd),
                parent.0,
                ord(pd),
            );
        }
        // All six gates pass. The child is stored into exactly this parent
        // (gate 4), so a prior insert can only have named the same parent —
        // idempotent.
        merged_parent.insert(child, parent);
    }
    merged_parent
}

/// A `letrec` closure-cycle merge (docs/impl/region/letrec.md § The letrec closure-cycle merge): one SCC of
/// mutually-recursive closures (plus a self-recursive member a sibling also
/// captures), plus their prebound capture cells, collapsed onto one arena and freed
/// by a single `DecrefRegion` at the cycle's binding scope. A *purely* self-recursive
/// closure is cell-free (its self-edge does not mark it captured — `hir/analyze/scopes.rs`)
/// and so never reaches the merge; the merge serves the cell-bearing cases.
pub(super) struct ClosureCycleMerge {
    /// The canonical root every member stars onto (its `merged_root`).
    pub root: Region,
    /// Every member region — the SCC closures and their cells, the root included.
    pub members: Vec<Region>,
    /// The cycle's binding scope — the non-lambda `Let`/`Letrec` that prebinds every
    /// member's capture cell. Its scope-exit frees the merged arena: it post-dominates
    /// every direct (binding-scoped) use of the members, while a foreign capture of a
    /// member is RC-counted and outlives the single decref (structural ancestry, never
    /// a numeric `ord` compare — docs/impl/region/adopt.md § The lifetime obligation
    /// the root carries).
    pub drop_site: HirId,
    /// The HirIds of the letrec body's tail calls to a **non-member** callee — the
    /// sites whose binding-scope `DecrefRegion` is stranded past a frame-replacing
    /// `TailCall` with no member-adopt channel. The lowerer keys `adopt_region_slot`
    /// (this cycle's `root` slot) at each so a closure callee's frame replacement is
    /// balanced by the activation-completion adopt (a native callee falls through to
    /// the live scope-exit drop). Empty when the body has no tail call, or only
    /// member-callee tail calls (which ride `stranded_cycle_bindings` instead).
    /// Recorded in `RegionInfo::cycle_tail_adopt` keyed to this `root`.
    pub tail_adopt_sites: Vec<HirId>,
}

/// One tail call in a `letrec` body, as the closure-cycle merge's tail gate reads
/// it (docs/impl/region/letrec.md § The letrec closure-cycle merge). A tail call
/// replaces the frame, stranding the merged arena's binding-scope `DecrefRegion`;
/// the gate must know both the callee and whether any cycle MEMBER flows in as an
/// argument, so a member passed by-move (`(g od)`) is refused — its own
/// move/return machinery would decref the arena a second time, colliding with the
/// adopt (a double-free), where a member merely STORED into a fresh aggregate then
/// passed is RC-counted and safe (after ANF the argument is a temp, not a member
/// reference, so it is admitted).
struct TailCallSite {
    /// The tail-call `Call` node's HirId — the key the lowerer sets
    /// `adopt_region_slot` at for a non-member callee.
    hir_id: HirId,
    /// The callee, unwrapped through `functionalize`'s `DerefCell`: `Some(b)` for a
    /// binding reference (a member, a native, a redefined operator, a foreign fn),
    /// `None` for a callee the gate cannot resolve (which refuses — no site to key
    /// the adopt at).
    callee: Option<crate::hir::Binding>,
    /// Every binding referenced in the tail call's ARGUMENT subtrees (Var reads,
    /// including a cell read's inner `Var`; nested lambdas are not descended). A
    /// member passed by-move is exactly a binding here whose source region is in the
    /// SCC. After ANF a nested aggregate/call argument is a fresh temp whose source
    /// region is the aggregate/call-result region — never the member's — so the
    /// RC-safe stored-then-passed case is not caught here (correctly admitted).
    arg_bindings: Vec<crate::hir::Binding>,
}

/// Detect the mergeable `letrec` closure cycles (docs/impl/region/letrec.md § The letrec closure-cycle merge).
///
/// A `letrec` mutually-recursive closure is a capture-cell↔closure cycle: each
/// member's prebound forward-reference cell holds the closure (`StoreCaptureCell`) and
/// the sibling closures capture the cell. Per-region RC cannot collect the immutable
/// cycle (region/rules.md Rule 8); the merge instead collapses the whole SCC ∪ its
/// cells onto one region, so the interior cell↔closure references become intra-region —
/// the alloc-scan incref, the capture-store incref, and the free-time cascade all
/// self-skip same-region refs (`regionpool/introspect.rs` `rid != own_id`,
/// `value/arena/mutate.rs::capture_store_with_rebind`), so the arena's RC is 1 and
/// one `DecrefRegion` frees the cycle wholesale. A *purely* self-recursive closure is
/// cell-free — the self-edge does not mark it captured (`hir/analyze/scopes.rs`), so it
/// has no forward cell and its self-reference resolves to the executing closure
/// (`LoadSelf` / a self-call), never a cell — so there is no cell↔closure cycle for the
/// merge to collapse; it is reclaimed by ordinary RC / the tail-call adopt, RC-identical
/// to a top-level recursive `defn`.
///
/// Two-layer detection. The **closures** carry the cycle: a `closure ⊇ closure`
/// capture graph with the `r == closure_r` self-edge ADMITTED (the very edge
/// `capture_containment_edges` drops). For a genuine mutual cycle the self-edge is
/// redundant (the sibling edges already close the SCC); it is load-bearing for the one
/// mixed shape that still has a cell — a self-recursive member a sibling ALSO captures
/// (so it keeps a cell for that sibling) but that is not itself in a mutual cycle: a
/// size-1 SCC the self-edge admits, whose cell the merge then collapses into the
/// closure (`merge_collapses_self_and_sibling_captured_member_cell`). The **cells** are
/// coincident-lifetime members, each paired in from its binding's `begin_cell_regions`
/// cell. A cycle is mergeable only when every closure is **non-escaping**
/// (`lambda_escapes_definition` false — a returned closure outlives the activation),
/// every member is **sole-held**, and every closure has a **static-slot cell**. The
/// cell requirement is met in every position: an immutable, lambda-initialized letrec
/// binding's forward cell is a compiled `MakeCaptureCell` at top level AND inside a
/// lambda body (`BindingInner::letrec_compiled_cell`). A mutated/reassigned in-lambda
/// letrec binding keeps the runtime env-cell route (no `begin_cell_regions` cell) and
/// is refused here to Shared, the always-legal baseline; a purely self-recursive
/// member has no cell at all and is likewise never a member. Two further gates:
/// every member's allocation must lie within the binding-scope letrec's own subtree
/// (so the drop site is a structural ancestor-or-self of every member), and the
/// letrec BODY's tail calls must each have a **release channel** for the merged
/// arena's binding-scope drop, which a frame-replacing tail call strands as dead
/// code. Two channels: a MEMBER callee rides `stranded_cycle_bindings` →
/// `tail_callee_adopts` (`region_of(callee)` is the arena); a NON-member callee — a
/// native, a redefined operator, a foreign fn — rides the explicit `adopt_region_slot`
/// (this cycle's root slot, recorded in `tail_adopt_sites`), so a closure callee's
/// frame replacement is balanced by the activation-completion adopt while a native
/// callee falls through to the live scope-exit drop (the two are mutually exclusive
/// per call, so exactly one release fires — the compiler never classifies the callee).
/// A non-member tail is refused only when its callee is unresolvable (no site to key
/// the adopt at) or when a cycle MEMBER flows into it **by-move** as an argument
/// (`(g od)`): the member's own move/return machinery would decref the arena a second
/// time, colliding with the adopt (a double-free). A member merely STORED into a fresh
/// aggregate then passed is RC-counted and safe, and after ANF is a temp argument, not
/// a member reference, so it is admitted. The result extends the same `merged_parent`
/// forest the builder-idiom seed populates and rides the same `merged_root`
/// canonicalization, unconditionally (not flag-gated) and on every tier.
pub(super) fn compute_closure_cycle_merges(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    order: &HashMap<HirId, u32>,
) -> Vec<ClosureCycleMerge> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    // Closure regions and region → lambda HirId (for the escape gate and drop site).
    let mut lambda_of: FxHashMap<Region, HirId> = FxHashMap::default();
    collect_closures(hir, info, &mut lambda_of);
    if lambda_of.is_empty() {
        return Vec::new();
    }
    let closure_regs: FxHashSet<Region> = lambda_of.keys().copied().collect();

    // The non-escape gate. A closure escapes its activation only by crossing a
    // FRONTIER (return / emit / send) — the Shared-seed set, which deliberately
    // EXCLUDES the capture facet (a captured-but-not-returned closure stays inside its
    // subtree). `lambda_escapes_definition` is the wrong gate here: it additionally
    // folds in the capture facet (a value captured by an escaping closure), a
    // CONTAINMENT relation — and a `letrec` SCC's closures capture each other, so one
    // member crossing a frontier would propagate "escaping" around the whole cycle and
    // over-refuse a mergeable one. The frontier question is exactly `compute_shared_seeds`.
    let shared = super::ownership::compute_shared_seeds(info, escape);

    // closure ⊇ closure capture edges (self-edges KEPT), restricted to closure regions:
    // the cycle a `letrec` forms lives entirely among the closure regions.
    let mut succ: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
    collect_closure_capture_edges(hir, info, &closure_regs, &mut succ);

    // closure region → its prebound capture cell (via `begin_cell_regions` and the
    // binding's source closure region); and every member region → its allocation HirId
    // (a closure's lambda, a cell's `Begin`/`Letrec`) for the drop site and root order.
    let mut cell_of: FxHashMap<Region, Region> = FxHashMap::default();
    let mut alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
    for (&r, &lid) in &lambda_of {
        alloc_hir.insert(r, lid);
    }
    for (&begin_id, cells) in &info.begin_cell_regions {
        for &(b, cell_r) in cells {
            alloc_hir.insert(cell_r, begin_id);
            if let Some(rs) = info.binding_source_regions.get(&b) {
                for &cr in rs {
                    if closure_regs.contains(&cr) {
                        cell_of.insert(cr, cell_r);
                    }
                }
            }
        }
    }

    // Sole-held index (any non-synthetic user binding is a holder), shared with the
    // merge seed and the ownership walks.
    let region_holders = super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );
    let sole_held =
        |r: Region| -> bool { region_holders.holders_of(r).is_none_or(|hs| hs.len() <= 1) };

    // Post-order subtree lower bounds, for the letrec-subtree containment gate
    // (an interval test `[low, order]` over the drop-site letrec), and each
    // letrec's body tail callees, for the tail gate. Both built once.
    let low = compute_subtree_low(hir, order);
    let mut letrec_tail: FxHashMap<HirId, Vec<TailCallSite>> = FxHashMap::default();
    collect_letrec_tail_callees(hir, &mut letrec_tail);

    // Transitive reach over the capture graph (a set closure, so a cycle terminates).
    let reach = |start: Region| -> FxHashSet<Region> {
        let mut set: FxHashSet<Region> = FxHashSet::default();
        set.insert(start);
        let mut work = vec![start];
        while let Some(n) = work.pop() {
            if let Some(kids) = succ.get(&n) {
                for &c in kids {
                    if set.insert(c) {
                        work.push(c);
                    }
                }
            }
        }
        set
    };

    // Iterate closures in program order so the SCC discovery (and the refusal set) is
    // deterministic across compiles.
    let mut ordered: Vec<Region> = closure_regs.iter().copied().collect();
    ordered.sort_by_key(|r| ord(alloc_hir[r]));
    let mut claimed: FxHashSet<Region> = FxHashSet::default();
    let mut out: Vec<ClosureCycleMerge> = Vec::new();
    for r in ordered {
        if claimed.contains(&r) {
            continue;
        }
        // The SCC of `r`: regions mutually reachable with it over the capture graph.
        let reach_r = reach(r);
        let scc: FxHashSet<Region> = reach_r
            .iter()
            .copied()
            .filter(|&m| reach(m).contains(&r))
            .collect();
        let self_edge = succ.get(&r).is_some_and(|s| s.contains(&r));
        // A genuine cycle: a multi-closure SCC, or a self-recursive closure (self-edge).
        if scc.len() < 2 && !self_edge {
            continue;
        }
        // Process each SCC once, accepted or refused.
        for &c in &scc {
            claimed.insert(c);
        }
        // Gate every closure: non-escaping, sole-held, with a sole-held static-slot
        // cell. Any failure refuses the whole SCC to Shared (the always-legal baseline).
        let mut members: Vec<Region> = Vec::with_capacity(scc.len() * 2);
        let mut ok = true;
        for &c in &scc {
            let Some(&cell_r) = cell_of.get(&c) else {
                ok = false;
                break;
            };
            if shared.contains(&c)
                || shared.contains(&cell_r)
                || !sole_held(c)
                || !sole_held(cell_r)
            {
                ok = false;
                break;
            }
            members.push(c);
            members.push(cell_r);
        }
        if !ok {
            continue;
        }
        // Drop site: the cycle's BINDING SCOPE — the single non-lambda Let/Letrec that
        // prebinds every member's capture cell (the `begin_cell_regions` key, recorded
        // in `alloc_hir` for each cell). Its scope-exit post-dominates every DIRECT
        // (binding-scoped) use of the members — they are bound there — so freeing the
        // cycle's own allocation reference there is sound and prompt. It is strictly
        // tighter than the allocation-site enclosing post-dominator (which excludes
        // the binding node from its own ancestor stack, dragging a top-level cycle's
        // drop up to the file Begin, i.e. program teardown); the binding-scope drop
        // frees a discarded cycle promptly instead (pinned by
        // `closure_cycle_discarded_release_is_prompt`, src/runtime/tests/ownership.rs).
        // A FOREIGN capture of a member (a closure outside the
        // SCC that holds it) is a cross-region reference INTO the merged arena, RC-counted
        // — increfed when the capturing closure is built (`incref_cross_region_refs`, which
        // also records the outgoing edge) and released by the free-time cascade walking that
        // recorded edge when the capturer's region frees — so it
        // survives the single decref until its capturer dies: the binding-scope drop
        // never frees a still-referenced arena. Members spanning >1 binding scope are
        // never a real SCC — exactly one letrec binds a mutual cycle — and refuse.
        let cell_scopes: FxHashSet<HirId> = scc
            .iter()
            .filter_map(|c| cell_of.get(c))
            .filter_map(|cr| alloc_hir.get(cr).copied())
            .collect();
        if cell_scopes.len() != 1 {
            continue;
        }
        let drop_site = cell_scopes.into_iter().next().unwrap();
        // Eligibility gate: LETREC-SUBTREE CONTAINMENT, decided structurally over the
        // post-order subtree interval `[low, order]` (never a bare numeric compare —
        // region/adopt.md § The lifetime obligation the root carries). Every member's
        // allocation site must lie within the binding-scope letrec's own subtree: a
        // cell's site IS the letrec node, a closure's Lambda is an init descendant —
        // so the drop site is a structural ancestor-or-self of every member by
        // construction, and a region reaching the SCC from OUTSIDE that subtree (a
        // reused binding identity naming a foreign lambda) refuses the cycle.
        let drop_lo = low.get(&drop_site).copied().unwrap_or(0);
        let drop_ord = ord(drop_site);
        let contained = members.iter().all(|m| {
            alloc_hir
                .get(m)
                .is_some_and(|&a| (drop_lo..=drop_ord).contains(&ord(a)))
        });
        if !contained {
            continue;
        }
        // Tail gate: every tail call in the letrec BODY (never inside a nested
        // lambda — those run in their own activations) must have a release channel
        // for the merged arena's binding-scope drop, which a frame-replacing
        // `TailCall` strands as dead code. A MEMBER callee rides the existing
        // stranded-cycle adopt (`stranded_cycle_bindings` → `tail_callee_adopts`,
        // `lir/lower/binding.rs`). A NON-member callee rides the explicit
        // `adopt_region_slot` (recorded below) — admissible only when the callee is
        // resolvable (a site to key the adopt at) AND no cycle member flows into the
        // tail call BY-MOVE as an argument: a member passed by-value (`(g od)`) has
        // its own move/return machinery decref the arena a SECOND time, colliding
        // with the adopt (a double-free), where a member stored into a fresh
        // aggregate then passed is RC-counted and (after ANF) a temp argument, so it
        // is admitted. Any tail call failing both channels refuses the cycle to
        // Shared (the always-legal baseline).
        let sites = letrec_tail.get(&drop_site);
        let is_member = |b: crate::hir::Binding| -> bool {
            info.binding_source_regions
                .get(&b)
                .is_some_and(|rs| rs.iter().any(|r| scc.contains(r)))
        };
        let strands = sites.is_none_or(|sites| {
            !sites.iter().all(|site| {
                if site.callee.is_some_and(is_member) {
                    return true; // member callee → existing stranded-cycle adopt
                }
                // Non-member (or unresolvable) callee → explicit arena adopt.
                site.callee.is_some() && !site.arg_bindings.iter().copied().any(is_member)
            })
        });
        if strands {
            continue;
        }
        // Every non-member-callee body tail is an admitted adopt site (a member
        // callee stays on its own channel and is excluded). Keyed to the root below.
        let tail_adopt_sites: Vec<HirId> = sites
            .map(|sites| {
                sites
                    .iter()
                    .filter(|site| !site.callee.is_some_and(is_member))
                    .map(|site| site.hir_id)
                    .collect()
            })
            .unwrap_or_default();
        // Numeric shadow of the structural ancestry: the binding-scope drop has the
        // highest post-order index in its subtree, so it dominates every member's
        // allocation (a cell's alloc HirId IS the drop site; a closure's is a strict
        // descendant). A future drift to a body-internal drop point detonates here in
        // debug rather than as a guardfree stale deref.
        #[cfg(debug_assertions)]
        {
            let drop_ord = ord(drop_site);
            for m in &members {
                if let Some(&a) = alloc_hir.get(m) {
                    debug_assert!(
                        ord(a) <= drop_ord,
                        "closure-cycle drop site @{} must post-dominate member r{}'s \
                         allocation @{}",
                        drop_site.0,
                        m.0,
                        a.0,
                    );
                }
            }
        }
        // Root: the SCC closure with the smallest program order — distinct per lambda,
        // so deterministic (region ids order nothing). Any member mints the shared
        // physical region at runtime (mint-or-reuse); the root only names the merged slot
        // and carries the single decref (set to `drop_site` by the caller).
        let root = *scc.iter().min_by_key(|&&c| ord(alloc_hir[&c])).unwrap();
        out.push(ClosureCycleMerge {
            root,
            members,
            drop_site,
            tail_adopt_sites,
        });
    }
    out
}

/// Collect each `Lambda`'s closure region (`alloc_region`) → its HirId.
fn collect_closures(hir: &Hir, info: &RegionInfo, out: &mut FxHashMap<Region, HirId>) {
    if matches!(hir.kind, HirKind::Lambda { .. }) {
        if let Some(&r) = info.alloc_region.get(&hir.id) {
            out.insert(r, hir.id);
        }
    }
    hir.for_each_child(|c| collect_closures(c, info, out));
}

/// Collect `closure → captured-closure` capture edges into `succ`, KEEPING the
/// `r == closure_r` self-edge and restricting to closure regions. Mirrors
/// `capture_containment_edges`' live-region filter but admits the self-edge that scan
/// drops. The self-edge is redundant for a genuine mutual cycle (the sibling edges
/// already close the SCC); it is load-bearing only for the mixed shape — a
/// self-recursive member a sibling ALSO captures, a size-1 SCC whose retained
/// (sibling-owned) cell the merge collapses via this self-edge
/// (`compute_closure_cycle_merges`).
fn collect_closure_capture_edges(
    hir: &Hir,
    info: &RegionInfo,
    closure_regs: &FxHashSet<Region>,
    succ: &mut FxHashMap<Region, FxHashSet<Region>>,
) {
    if let HirKind::Lambda { captures, .. } = &hir.kind {
        if let Some(&closure_r) = info.alloc_region.get(&hir.id) {
            for c in captures {
                if let Some(regions) = info.binding_source_regions.get(&c.binding) {
                    for &r in regions {
                        if info.live_regions.contains(&r) && closure_regs.contains(&r) {
                            succ.entry(closure_r).or_default().insert(r);
                        }
                    }
                }
            }
        }
    }
    hir.for_each_child(|c| collect_closure_capture_edges(c, info, closure_regs, succ));
}

/// For every `Letrec` node, one [`TailCallSite`] per tail call in its BODY. Feeds
/// the closure-cycle merge's tail gate: a body tail call replaces the frame,
/// stranding the binding-scope drop, so each must supply a release channel (a
/// member callee's stranded-cycle adopt, or a non-member callee's explicit arena
/// adopt) and must not pass a cycle member in by-move.
fn collect_letrec_tail_callees(hir: &Hir, out: &mut FxHashMap<HirId, Vec<TailCallSite>>) {
    if let HirKind::Letrec { body, .. } = &hir.kind {
        let mut sites = Vec::new();
        body_tail_callees(body, &mut sites);
        out.insert(hir.id, sites);
    }
    hir.for_each_child(|c| collect_letrec_tail_callees(c, out));
}

/// The [`TailCallSite`]s within one letrec body. Never descends into a `Lambda`
/// — a nested closure's tail calls run in that closure's own activation, not the
/// letrec's, so they neither strand nor may adopt the merged arena (mirrors the
/// lowerer's `collect_body_tail_callees`, `lir/lower/binding.rs`). The callee is
/// unwrapped through the `DerefCell` `functionalize` adds around a needs-capture
/// binding read; each argument subtree contributes its referenced bindings.
fn body_tail_callees(hir: &Hir, out: &mut Vec<TailCallSite>) {
    if matches!(hir.kind, HirKind::Lambda { .. }) {
        return;
    }
    if let HirKind::Call {
        func,
        args,
        is_tail: true,
        ..
    } = &hir.kind
    {
        let callee_node = match &func.kind {
            HirKind::DerefCell { cell } => cell,
            _ => func,
        };
        let callee = match &callee_node.kind {
            HirKind::Var(b) => Some(*b),
            _ => None,
        };
        let mut arg_bindings = Vec::new();
        for a in args {
            arg_flow_bindings(&a.expr, &mut arg_bindings);
        }
        out.push(TailCallSite {
            hir_id: hir.id,
            callee,
            arg_bindings,
        });
    }
    hir.for_each_child(|c| body_tail_callees(c, out));
}

/// The bindings a tail-call argument region-transparently evaluates **to** — the
/// values that flow BY-MOVE into the tail call. This mirrors escape's `tail_sources`
/// descent (`hir/escape/flow.rs`): pass through the pure control / select / deref /
/// bind wrappers, but STOP at a `Call`, an `Intrinsic`, and a `Lambda`. A member
/// reached only past a stopped node is NOT by-move:
///
///  - a nested `Call` — `(g (ev k))` — has `ev` as its callee, so `ev`'s RESULT (a
///    value) flows, not `ev` itself; a member passed as a nested-call argument is
///    incref-balanced (a non-tail call owns its params), not moved;
///  - an `Intrinsic` — `(g (%pair od 1))` — stores the member into a fresh aggregate,
///    an RC-counted reference the aggregate's cascade releases;
///  - a `Lambda` — a closure argument's captures are RC-counted.
///
/// so none collides with the merged-arena adopt. Only a bare member value in a direct
/// argument (`(g od)`, or through an `If`/`Begin`/`DerefCell` that selects one) is
/// moved with no incref, and its own move/return decref would double-free the arena —
/// which the tail gate reads this to refuse.
fn arg_flow_bindings(hir: &Hir, out: &mut Vec<crate::hir::Binding>) {
    match &hir.kind {
        HirKind::Var(b) => out.push(*b),
        HirKind::DerefCell { cell } => arg_flow_bindings(cell, out),
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => arg_flow_bindings(body, out),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            arg_flow_bindings(then_branch, out);
            arg_flow_bindings(else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                arg_flow_bindings(b, out);
            }
            if let Some(eb) = else_branch {
                arg_flow_bindings(eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                arg_flow_bindings(last, out);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                arg_flow_bindings(e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                arg_flow_bindings(body, out);
            }
        }
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => arg_flow_bindings(value, out),
        // A Call / Intrinsic / Lambda / immediate: a fresh, incref-balanced, or
        // RC-counted result — no member flows by-move. Stop.
        _ => {}
    }
}

/// Collect every `%pair` (`IntrinsicOp::Pair`) node's HirId.
fn collect_pair_sites(hir: &Hir, out: &mut FxHashSet<HirId>) {
    if let HirKind::Intrinsic {
        op: crate::hir::expr::IntrinsicOp::Pair,
        ..
    } = &hir.kind
    {
        out.insert(hir.id);
    }
    hir.for_each_child(|c| collect_pair_sites(c, out));
}
