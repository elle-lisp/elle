//! The `letrec` closure-cycle merge (docs/impl/region/letrec.md § The letrec
//! closure-cycle merge): one SCC of mutually-recursive closures ∪ their prebound
//! capture cells, collapsed onto one arena and freed by a single `DecrefRegion` at
//! the cycle's binding scope — or, where the letrec hands a member out, where that
//! member's own release already sits.

use super::super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// A `letrec` closure-cycle merge (docs/impl/region/letrec.md § The letrec closure-cycle merge): one SCC of
/// mutually-recursive closures (plus a self-recursive member a sibling also
/// captures), plus their prebound capture cells, collapsed onto one arena and freed
/// by a single `DecrefRegion` at the [`ClosureCycleMerge::drop_site`]. A *purely* self-recursive
/// closure is cell-free (its self-edge does not mark it captured — `hir/analyze/scopes.rs`)
/// and so never reaches the merge; the merge serves the cell-bearing cases.
pub(crate) struct ClosureCycleMerge {
    /// The canonical root every member stars onto (its `merged_root`).
    pub root: Region,
    /// Every member region — the SCC closures and their cells, the root included.
    pub members: Vec<Region>,
    /// Where the merged arena's single `DecrefRegion` fires. Normally the cycle's
    /// **binding scope** — the non-lambda `Let`/`Letrec` that prebinds every member's
    /// capture cell — whose scope-exit post-dominates every direct (binding-scoped) use
    /// of the members, while a foreign capture of a member is RC-counted and outlives
    /// the single decref. Where the letrec HANDS A MEMBER OUT (its body falls out to a
    /// bare member value), that value leaves the scope on an uncounted read, and this is
    /// instead the release point the last-use rule already computed for the handed-out
    /// member — a node post-dominating the binding scope from outside it
    /// (docs/impl/region/letrec.md § "Drop site — following a handed-out member").
    /// Either way decided by structural ancestry, never a numeric `ord` compare
    /// (docs/impl/region/adopt.md § The lifetime obligation the root carries).
    pub drop_site: HirId,
    /// The HirIds of the letrec body's tail calls to a **non-member** callee — the
    /// sites whose binding-scope `DecrefRegion` is stranded past a frame-replacing
    /// `TailCall` with no member-deferral channel. The lowerer keys `deferred_release_slot`
    /// (this cycle's `root` slot) at each so a closure callee's frame replacement is
    /// balanced by the activation-completion deferred release (a native callee falls through to
    /// the live scope-exit drop). Empty when the body has no tail call, or only
    /// member-callee tail calls (which ride `stranded_cycle_bindings` instead).
    /// Recorded in `RegionInfo::cycle_tail_release` keyed to this `root`.
    pub tail_release_sites: Vec<HirId>,
}

/// One tail call in a `letrec` body, as the closure-cycle merge's tail gate reads
/// it (docs/impl/region/letrec.md § The letrec closure-cycle merge). A tail call
/// replaces the frame, stranding the merged arena's binding-scope `DecrefRegion`;
/// the gate must know both the callee and whether any cycle MEMBER flows in as an
/// argument, so a member passed by-move (`(g od)`) is refused — its own
/// move/return machinery would decref the arena a second time, colliding with the
/// deferred release (a double-free), where a member merely STORED into a fresh aggregate then
/// passed is RC-counted and safe (after ANF the argument is a temp, not a member
/// reference, so it is admitted).
struct TailCallSite {
    /// The tail-call `Call` node's HirId — the key the lowerer sets
    /// `deferred_release_slot` at for a non-member callee.
    hir_id: HirId,
    /// The callee, unwrapped through `functionalize`'s `DerefCell`: `Some(b)` for a
    /// binding reference (a member, a native, a redefined operator, a foreign fn),
    /// `None` for a callee the gate cannot resolve (which refuses — no site to key
    /// the deferred release at).
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
/// merge to collapse; it is reclaimed by ordinary RC / the tail-call deferred release, RC-identical
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
/// cell. A cycle is mergeable only when every member is **sole-held** (waived for a
/// member the letrec hands out, whose adopted release point below counts every holder
/// directly), every closure has a **static-slot cell**, and every closure clears the
/// **frontier gate**: the FIBER half (emit / send) refuses outright, while the RETURN
/// half is admitted where the arena's release provably runs after the mint that funds
/// the consumer (docs/impl/region/letrec.md § The frontier gate). The
/// cell requirement is met in every position: an immutable, lambda-initialized letrec
/// binding's forward cell is a compiled `MakeCaptureCell` at top level AND inside a
/// lambda body (`BindingInner::letrec_compiled_cell`). A mutated/reassigned in-lambda
/// letrec binding keeps the runtime env-cell route (no `begin_cell_regions` cell) and
/// is refused here to Shared, the always-legal baseline; a purely self-recursive
/// member has no cell at all and is likewise never a member. Two further gates:
/// every member's allocation must lie within the binding-scope letrec's own subtree
/// (so that scope is a structural ancestor-or-self of every member), and the
/// letrec BODY's tail calls must each have a **release channel** for the merged
/// arena's binding-scope drop, which a frame-replacing tail call strands as dead
/// code. Two channels: a MEMBER callee rides `stranded_cycle_bindings` →
/// `tail_callee_defers_release` (`region_of(callee)` is the arena); a NON-member callee — a
/// native, a redefined operator, a foreign fn — rides the explicit `deferred_release_slot`
/// (this cycle's root slot, recorded in `tail_release_sites`), so a closure callee's
/// frame replacement is balanced by the activation-completion deferred release while a native
/// callee falls through to the live scope-exit drop (the two are mutually exclusive
/// per call, so exactly one release fires — the compiler never classifies the callee).
/// A non-member tail is refused only when its callee is unresolvable (no site to key
/// the deferred release at) or when a cycle MEMBER flows into it **by-move** as an argument
/// (`(g od)`): the member's own move/return machinery would decref the arena a second
/// time, colliding with the deferred release (a double-free). A member merely STORED into a fresh
/// aggregate then passed is RC-counted and safe, and after ANF is a temp argument, not
/// a member reference, so it is admitted. When a member carries the return facet the body
/// is read once more, for a different question — where the mint that funds the consumer
/// stands relative to the arena's release. Either every tail EXIT of the body LEAVES THE
/// FRAME (a `Return`, or a tail `Call`), putting that mint inside the body and so ahead
/// of a release at the binding scope; or the body falls out to a bare member value, and
/// the release instead FOLLOWS THAT MEMBER to the point the last-use rule already
/// computed for it, which post-dates every consumer's claim. A body doing neither keeps
/// the Shared baseline. The result extends the same `merged_parent` forest the
/// builder-idiom seed populates and rides the same `merged_root` canonicalization,
/// unconditionally (not flag-gated) and on every tier.
pub(crate) fn compute_closure_cycle_merges(
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

    // The frontier gate (docs/impl/region/letrec.md § The frontier gate), read as its
    // two halves rather than the combined Shared-seed set. A closure escapes its
    // activation only by crossing a FRONTIER (return / emit / send); the capture facet
    // is deliberately excluded, because `lambda_escapes_definition` folds in a
    // CONTAINMENT relation — and a `letrec` SCC's closures capture each other, so one
    // member crossing a frontier would propagate "escaping" around the whole cycle and
    // over-refuse a mergeable one.
    //
    // The halves are then admitted differently, because the merge's release is a
    // decref, not a free. The FIBER half refuses outright: an emitted or sent member
    // reaches a holder the compiler did not place, and a parked frame may borrow it
    // uncounted, so no ordering argument funds it. The RETURN half is **return-funded**
    // — the merge collapses the returned member's region onto the arena, so the value
    // handed out lives in the arena and the callee's `Return` mint raises the arena's
    // own count — but only where the arena's release provably runs AFTER that mint,
    // which is the tail-shape gate further down.
    let fiber = super::super::escape::fiber_frontier_regions(escape, info);
    let returned = super::super::escape::return_frontier_regions(
        escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );

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
    let region_holders = super::super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );
    let sole_held =
        |r: Region| -> bool { region_holders.holders_of(r).is_none_or(|hs| hs.len() <= 1) };

    // Structural post-dominance over the scope tree, for the letrec-subtree containment
    // gate (an ancestry test over the binding scope) and for the handed-out member's
    // adopted release point (which must post-dominate that scope from outside it). Plus
    // each letrec's body tail reading, for the tail gates. Both built once.
    let pd = super::super::postdom::PostDom::new(hir, order);
    let mut letrec_tail: FxHashMap<HirId, LetrecTail> = FxHashMap::default();
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
        // The cycle's BINDING SCOPE — the single non-lambda Let/Letrec that prebinds
        // every member's capture cell (the `begin_cell_regions` key, recorded in
        // `alloc_hir` for each cell). Its scope-exit post-dominates every DIRECT
        // (binding-scoped) use of the members — they are bound there — so freeing the
        // cycle's own allocation reference there is sound and prompt. It is strictly
        // tighter than the allocation-site enclosing post-dominator (which excludes
        // the binding node from its own ancestor stack, dragging a top-level cycle's
        // drop up to the file Begin, i.e. program teardown); the binding-scope drop
        // frees a discarded cycle promptly instead (pinned by
        // `closure_cycle_discarded_release_is_prompt`, src/runtime/tests/ownership/).
        // A FOREIGN capture of a member (a closure outside the
        // SCC that holds it) is a cross-region reference INTO the merged arena, RC-counted
        // — increfed when the capturing closure is built (`incref_cross_region_refs`, which
        // also records the outgoing edge) and released by the free-time cascade walking that
        // recorded edge when the capturer's region frees — so it
        // survives the single decref until its capturer dies: the binding-scope drop
        // never frees a still-referenced arena. Members spanning >1 binding scope are
        // never a real SCC — exactly one letrec binds a mutual cycle — and refuse.
        // Computed before the member gates below, which consult the body's tail.
        let cell_scopes: FxHashSet<HirId> = scc
            .iter()
            .filter_map(|c| cell_of.get(c))
            .filter_map(|cr| alloc_hir.get(cr).copied())
            .collect();
        if cell_scopes.len() != 1 {
            continue;
        }
        let binding_scope = cell_scopes.into_iter().next().unwrap();
        let tail = letrec_tail.get(&binding_scope);
        let exits_frame = tail.is_some_and(|t| t.exits_frame);

        // The members the letrec HANDS OUT (docs/impl/region/letrec.md § "Drop site —
        // following a handed-out member"). Foreign capture is RC-counted, so the
        // letrec's own value is the one uncounted way a member leaves the binding scope
        // — and only when the body does not leave the frame itself, where that value is
        // the frame's own result and its mint is inside the body. An enclosing
        // consumer's binding then names the member's region directly (a `Var`/`DerefCell`
        // read mints nothing), which is both why the binding-scope release is too early
        // and why counting that binding as a second holder measures the wrong thing.
        let hands_out: FxHashSet<Region> = if exits_frame {
            FxHashSet::default()
        } else {
            tail.map(|t| {
                t.value_bindings
                    .iter()
                    .filter_map(|b| info.binding_source_regions.get(b))
                    .flatten()
                    .copied()
                    .filter(|r| scc.contains(r))
                    .collect()
            })
            .unwrap_or_default()
        };

        // Gate every closure: off the fiber frontier, sole-held, with a sole-held
        // static-slot cell. Any failure refuses the whole SCC to Shared (the
        // always-legal baseline). A member on the RETURN frontier does not refuse here;
        // it raises `return_facet`, which the ordering gate below then has to fund.
        //
        // Sole-heldness is WAIVED for a handed-out member. It is a proxy for "no second
        // name reaches this member", asked because a release pinned at the binding scope
        // cannot see past itself; where the release instead follows the value out, the
        // adopted point is computed over every holder's last use and is the real thing.
        // Cells and members the letrec does not hand out keep the proxy.
        let mut members: Vec<Region> = Vec::with_capacity(scc.len() * 2);
        let mut ok = true;
        let mut return_facet = false;
        for &c in &scc {
            let Some(&cell_r) = cell_of.get(&c) else {
                ok = false;
                break;
            };
            if fiber.contains(&c)
                || fiber.contains(&cell_r)
                || (!hands_out.contains(&c) && !sole_held(c))
                || !sole_held(cell_r)
            {
                ok = false;
                break;
            }
            return_facet |= returned.contains(&c) || returned.contains(&cell_r);
            members.push(c);
            members.push(cell_r);
        }
        if !ok {
            continue;
        }
        // Where the arena's single `DecrefRegion` fires. With nothing handed out that is
        // the binding scope. With a member handed out it must FOLLOW THE VALUE: the
        // member's region already carries the release point the last-use rule computed
        // over every holder, so adopting it as the arena's adds no release the program
        // did not have — it only widens what the one release covers, to members whose
        // uses all sit inside a scope that point post-dominates. Admitted when the
        // handed-out members agree on one point (one arena carries one release) that
        // lies OUTSIDE the binding scope and post-dominates it; a point inside means the
        // value never actually left, and anything else the reading cannot place keeps
        // the Shared baseline.
        let drop_site = if hands_out.is_empty() {
            binding_scope
        } else {
            let mut points: FxHashSet<HirId> = FxHashSet::default();
            let mut all_placed = true;
            for r in &hands_out {
                match info.region_data.get(r) {
                    Some(d) => {
                        points.insert(d.decref_point);
                    }
                    None => all_placed = false,
                }
            }
            if !all_placed || points.len() != 1 {
                continue;
            }
            let p = points.into_iter().next().unwrap();
            if pd.in_subtree(p, binding_scope) {
                binding_scope
            } else if pd.drop_post_dominates(
                p,
                binding_scope,
                super::super::postdom::EmitMode::Merge,
            ) {
                p
            } else {
                continue;
            }
        };
        // Eligibility gate: LETREC-SUBTREE CONTAINMENT, decided structurally over the
        // scope tree (never a bare numeric compare — region/adopt.md § The lifetime
        // obligation the root carries). Every member's allocation site must lie within
        // the binding-scope letrec's own subtree: a cell's site IS the letrec node, a
        // closure's Lambda is an init descendant — so the binding scope is a structural
        // ancestor-or-self of every member by construction, and a region reaching the
        // SCC from OUTSIDE that subtree (a reused binding identity naming a foreign
        // lambda) refuses the cycle. The drop site is that scope or a node
        // post-dominating it, so it inherits the property.
        let contained = members.iter().all(|m| {
            alloc_hir
                .get(m)
                .is_some_and(|&a| pd.in_subtree(a, binding_scope))
        });
        if !contained {
            continue;
        }
        // Tail gate: every tail call in the letrec BODY (never inside a nested
        // lambda — those run in their own activations) must have a release channel
        // for the merged arena's binding-scope drop, which a frame-replacing
        // `TailCall` strands as dead code. A MEMBER callee rides the existing
        // stranded-cycle deferral (`stranded_cycle_bindings` → `tail_callee_defers_release`,
        // `lir/lower/binding.rs`). A NON-member callee rides the explicit
        // `deferred_release_slot` (recorded below) — admissible only when the callee is
        // resolvable (a site to key the deferred release at) AND no cycle member flows into the
        // tail call BY-MOVE as an argument: a member passed by-value (`(g od)`) has
        // its own move/return machinery decref the arena a SECOND time, colliding
        // with the deferred release (a double-free), where a member stored into a fresh
        // aggregate then passed is RC-counted and (after ANF) a temp argument, so it
        // is admitted. Any tail call failing both channels refuses the cycle to
        // Shared (the always-legal baseline).
        let sites = tail.map(|t| &t.sites);
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
        // The RETURN-FUNDED admission's ordering requirement (docs/impl/region/letrec.md
        // § The frontier gate). A returned member's arena may be released only AFTER the
        // mint that funds the caller's reference, and one structural fact settles that
        // for every channel at once: the letrec BODY must hand the value over itself —
        // every tail exit of it leaves the frame. A `Return` mints where it stands,
        // inside the body, ahead of the binding-scope `DecrefRegion` the lowerer emits
        // at the `Letrec` node; a tail call to a closure replaces the frame, so that
        // drop is dead and the release rides a deferral `trampoline_loop` runs at the
        // recursion's normal completion, after the callee's `Return`; a tail call to a
        // native keeps the frame but mints at the call site (the post-`TailCall`
        // fall-through retain, or `lower_return`'s where ANF named the result), also
        // inside the body.
        //
        // A body that falls out to a bare VALUE hands the letrec's value to an
        // ENCLOSING consumer instead, so no mint stands inside the body. The order then
        // comes from the other side — `drop_site` above followed the handed-out member
        // to the release point that already post-dates every consumer's claim on it. A
        // body that does NEITHER gives the reading nothing to order against (its value
        // is a call result, or an exit the reading cannot place), and a returned member
        // there keeps the Shared baseline.
        if return_facet && !exits_frame && hands_out.is_empty() {
            continue;
        }
        // Every non-member-callee body tail is an admitted adopt site (a member
        // callee stays on its own channel and is excluded). Keyed to the root below.
        let tail_release_sites: Vec<HirId> = sites
            .map(|sites| {
                sites
                    .iter()
                    .filter(|site| !site.callee.is_some_and(is_member))
                    .map(|site| site.hir_id)
                    .collect()
            })
            .unwrap_or_default();
        // Numeric shadow of the structural ancestry: the binding scope has the highest
        // post-order index in its subtree, so it dominates every member's allocation (a
        // cell's alloc HirId IS that node; a closure's is a strict descendant), and a
        // drop site that post-dominates the scope from outside sequences after it again.
        // A future drift to a body-internal drop point detonates here in debug rather
        // than as a guardfree stale deref.
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
            tail_release_sites,
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

/// What one `Letrec`'s BODY looks like at its tail, as the merge's two tail gates
/// read it.
struct LetrecTail {
    /// One [`TailCallSite`] per tail call in the body — the release-channel gate's
    /// input. A body tail call replaces the frame, stranding the binding-scope drop,
    /// so each must supply a release channel (a member callee's stranded-cycle
    /// deferral, or a non-member callee's explicit arena adopt) and must not pass a
    /// cycle member in by-move.
    sites: Vec<TailCallSite>,
    /// Does EVERY tail exit of the body leave the frame itself — a `Return`, or a
    /// tail `Call`? The **return-funded** admission needs this reading rather than
    /// `sites`: it decides whether the value the frame hands its caller is minted
    /// INSIDE the body, ahead of the binding-scope drop the lowerer emits at the
    /// `Letrec` node (docs/impl/region/letrec.md § The frontier gate).
    exits_frame: bool,
    /// The bindings the body's tail region-transparently evaluates TO — the value the
    /// `Letrec` node itself yields. Read only where `exits_frame` is false, i.e. where
    /// that value goes to an ENCLOSING consumer: a cycle member among them leaves the
    /// binding scope on an uncounted read, so the arena's release follows it instead of
    /// staying at the scope (docs/impl/region/letrec.md § "Drop site — following a
    /// handed-out member").
    value_bindings: Vec<crate::hir::Binding>,
}

/// For every `Letrec` node, how its BODY exits at the tail ([`LetrecTail`]).
fn collect_letrec_tail_callees(hir: &Hir, out: &mut FxHashMap<HirId, LetrecTail>) {
    if let HirKind::Letrec { body, .. } = &hir.kind {
        let mut sites = Vec::new();
        body_tail_callees(body, &mut sites);
        let mut value_bindings = Vec::new();
        value_flow_bindings(body, &mut value_bindings);
        out.insert(
            hir.id,
            LetrecTail {
                sites,
                exits_frame: body_tail_exits_frame(body),
                value_bindings,
            },
        );
    }
    hir.for_each_child(|c| collect_letrec_tail_callees(c, out));
}

/// Does every tail EXIT of a letrec body leave the frame — is the value the frame
/// hands its caller produced (and minted) INSIDE this body?
///
/// The return-funded admission reads this to know that the merge's binding-scope
/// release runs after that mint (docs/impl/region/letrec.md § The frontier gate).
/// Descends only the tail position of the pure control forms, and deliberately answers
/// `false` for everything it does not recognise: a body it cannot read may hand its
/// value to an enclosing consumer, whose mint is outside the letrec node and therefore
/// after the release. A bare value tail out of tail position
/// (`(let [c (letrec [ev …] ev)] … c)`), a loop, a short-circuit `And`/`Or`, and a
/// `Cond` with no else arm all read that way.
fn body_tail_exits_frame(hir: &Hir) -> bool {
    match &hir.kind {
        // The two nodes that hand a value to the CALLER from inside this body: the
        // `Return` mints for it here, and a tail `Call` mints either at the callee's
        // own `Return` (a closure, which replaces the frame) or at the post-`TailCall`
        // fall-through retain emitted right here (a native, which does not).
        HirKind::Return { .. } | HirKind::Call { is_tail: true, .. } => true,
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Parameterize { body, .. } => body_tail_exits_frame(body),
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            exprs.last().is_some_and(body_tail_exits_frame)
        }
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => body_tail_exits_frame(then_branch) && body_tail_exits_frame(else_branch),
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // No else arm means an implicit nil fall-through — an exit that does not
            // leave the frame, so the whole `Cond` does not.
            else_branch
                .as_ref()
                .is_some_and(|e| body_tail_exits_frame(e))
                && clauses.iter().all(|(_, b)| body_tail_exits_frame(b))
        }
        // `Match` is absent deliberately, where branch compensation admits it exactly as
        // an `If`: this reading needs the arms to be EXHAUSTIVE (a fall-through with no
        // arm taken is an exit that does not leave the frame), which an `If` and a
        // `Cond` with an else arm have and a pattern match does not.
        _ => false,
    }
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
            value_flow_bindings(&a.expr, &mut arg_bindings);
        }
        out.push(TailCallSite {
            hir_id: hir.id,
            callee,
            arg_bindings,
        });
    }
    hir.for_each_child(|c| body_tail_callees(c, out));
}

/// The bindings an expression region-transparently evaluates **to** — the values that
/// flow out of it with no incref of their own. This mirrors escape's `tail_sources`
/// descent (`hir/escape/flow.rs`): pass through the pure control / select / deref /
/// bind wrappers, but STOP at a `Call`, an `Intrinsic`, and a `Lambda`. A member
/// reached only past a stopped node did not flow uncounted:
///
///  - a nested `Call` — `(g (ev k))` — has `ev` as its callee, so `ev`'s RESULT (a
///    value) flows, not `ev` itself; a member passed as a nested-call argument is
///    incref-balanced (a non-tail call owns its params), not moved;
///  - an `Intrinsic` — `(g (%pair od 1))` — stores the member into a fresh aggregate,
///    an RC-counted reference the aggregate's cascade releases;
///  - a `Lambda` — a closure argument's captures are RC-counted.
///
/// Two readers, asking the same question of different expressions. Over a **tail-call
/// argument** it names what flows BY-MOVE into the call: only a bare member value in a
/// direct argument (`(g od)`, or through an `If`/`Begin`/`DerefCell` that selects one)
/// is moved with no incref, and its own move/return decref would double-free the arena,
/// which the tail gate reads this to refuse. Over a **letrec body** it names the value
/// the `Letrec` node yields, so a member there is one the cycle hands to an enclosing
/// consumer — read only where the body does not leave the frame, hence never through
/// the `Return` arm below.
fn value_flow_bindings(hir: &Hir, out: &mut Vec<crate::hir::Binding>) {
    match &hir.kind {
        HirKind::Var(b) => out.push(*b),
        HirKind::DerefCell { cell } => value_flow_bindings(cell, out),
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => value_flow_bindings(body, out),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            value_flow_bindings(then_branch, out);
            value_flow_bindings(else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                value_flow_bindings(b, out);
            }
            if let Some(eb) = else_branch {
                value_flow_bindings(eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                value_flow_bindings(last, out);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                value_flow_bindings(e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                value_flow_bindings(body, out);
            }
        }
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => value_flow_bindings(value, out),
        // A Call / Intrinsic / Lambda / immediate: a fresh, incref-balanced, or
        // RC-counted result — no member flows by-move. Stop.
        _ => {}
    }
}
