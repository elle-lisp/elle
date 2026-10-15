// audited: 2026-10-14
//! The `letrec` closure-cycle merge: one SCC of mutually-recursive closures and their
//! prebound capture cells, collapsed onto one arena.
//!
//! docs/impl/region/letrec.md
//! docs/impl/region/adopt.md
//!
//! The single `DecrefRegion` fires at the cycle's binding scope, or, where the
//! scope hands a member out, where that member's own release already sits. [`body`]
//! reads the scope's body at its tail for the gates below.

mod body;

use super::super::*;
use body::{collect_scope_tails, ScopeTail};
use rustc_hash::{FxHashMap, FxHashSet};

/// One admitted `letrec` closure-cycle merge: an SCC of mutually-recursive closures
/// (plus a self-recursive member a sibling also captures) and their prebound capture
/// cells, collapsed onto one arena and freed by a single `DecrefRegion` at the
/// [`drop_site`](ClosureCycleMerge::drop_site).
pub(crate) struct ClosureCycleMerge {
    /// The canonical root every member stars onto (its `merged_root`).
    pub root: Region,
    /// Every member region — the SCC closures and their cells, the root included.
    pub members: Vec<Region>,
    /// Where the merged arena's single `DecrefRegion` fires. Normally the cycle's
    /// **binding scope**, the node that prebinds every member's capture cell. Where
    /// the scope's body hands a member out, it is the release point that member's
    /// region already carries, a node outside the scope that post-dominates it.
    /// Structural ancestry decides it, never a numeric `ord` compare.
    pub drop_site: HirId,
    /// The scope body's tail calls to a **non-member** callee, whose binding-scope
    /// `DecrefRegion` a frame-replacing `TailCall` strands. The lowerer keys
    /// `deferred_release_slot` (this cycle's `root` slot) at each, so a closure
    /// callee's activation releases the arena at completion. A native callee falls
    /// through to the live scope-exit drop. A member callee rides
    /// `stranded_cycle_bindings` instead and is absent here. The caller records
    /// these in `RegionInfo::cycle_tail_release`, keyed to `root`.
    pub tail_release_sites: Vec<HirId>,
}

/// Detect the mergeable `letrec` closure cycles.
///
/// Detection has two layers, because the cell↔closure structure is not one SCC in
/// the graphs the other passes build. The **closures** carry the cycle: a
/// `closure ⊇ closure` capture graph that keeps the `r == closure_r` self-edge
/// `capture_containment_edges` drops. The self-edge admits the one mixed shape with a
/// cell and no mutual cycle, a self-recursive member a sibling also captures. The
/// **cells** are paired in from each member binding's `begin_cell_regions` entry. A
/// member with no static-slot cell, such as a mutated in-lambda letrec binding,
/// refuses the whole SCC.
///
/// The binding scope is the node that prebinds the cells: the `letrec`, or the
/// `begin` a run of local `defn`s sits in. One predicate answers for both
/// (`BindingInner::compiled_forward_cell`).
///
/// The gates, in the order the loop asks them:
///
///  1. ONE binding scope for the whole SCC — the node prebinding every cell;
///  2. per member: off the FIBER frontier, sole-held (waived for a handed-out
///     member), with a sole-held static-slot cell;
///  3. the DROP SITE — that binding scope, or the release point a handed-out
///     member's own region already carries;
///  4. BINDING-SCOPE-SUBTREE CONTAINMENT of every member's allocation site;
///  5. a RELEASE CHANNEL for every tail call in that scope's body;
///  6. for a member on the RETURN frontier, an order that releases the arena after
///     the mint that funds the caller.
///
/// A cycle failing any gate keeps the Shared baseline, which is always legal. The
/// caller extends the builder-idiom seed's `merged_parent` forest with the result,
/// on every tier.
pub(crate) fn compute_closure_cycle_merges(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    order: &Order,
) -> Vec<ClosureCycleMerge> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    // Each closure region → the `Lambda` that allocates it.
    let mut lambda_of: FxHashMap<Region, HirId> = FxHashMap::default();
    collect_closures(hir, info, &mut lambda_of);
    if lambda_of.is_empty() {
        return Vec::new();
    }
    let closure_regs: FxHashSet<Region> = lambda_of.keys().copied().collect();

    // The frontier gate reads its two halves apart, and not through
    // `lambda_escapes_definition`: that method folds in a containment relation, and
    // an SCC's closures capture each other, so one member crossing a frontier would
    // refuse the whole cycle. The fiber half refuses in gate 2; the return half sets
    // `return_facet`, which gate 6 must fund.
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
    // (a closure's lambda, a cell's `Begin`/`Letrec`) for gate 4 and the root order.
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
    let mut scope_tail: FxHashMap<HirId, ScopeTail> = FxHashMap::default();
    collect_scope_tails(hir, &mut scope_tail);

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
        // Gate 1: the cycle's BINDING SCOPE, the one `Letrec` or `Begin` that
        // prebinds every member's capture cell (the `begin_cell_regions` key, which
        // `alloc_hir` records for each cell). Its scope exit post-dominates every
        // direct use of the members. A foreign capture of a member is RC-counted,
        // so a drop there never frees a still-referenced arena. Cells that name
        // more than one scope refuse. The later gates read this scope's body tail.
        let cell_scopes: FxHashSet<HirId> = scc
            .iter()
            .filter_map(|c| cell_of.get(c))
            .filter_map(|cr| alloc_hir.get(cr).copied())
            .collect();
        if cell_scopes.len() != 1 {
            continue;
        }
        let binding_scope = cell_scopes.into_iter().next().unwrap();
        let tail = scope_tail.get(&binding_scope);
        let exits_frame = tail.is_some_and(|t| t.exits_frame);

        // The members the scope HANDS OUT as its own value. That value leaves the
        // scope uncounted, because a `Var` or `DerefCell` read mints nothing. A body
        // that leaves the frame itself mints its value inside the body, so it hands
        // out nothing here.
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

        // Gate 2, per closure: off the fiber frontier, sole-held, with a sole-held
        // static-slot cell. A failure refuses the whole SCC to Shared. A member on
        // the return frontier sets `return_facet` instead, which gate 6 must fund.
        // A handed-out member skips the sole-held test: its release follows the
        // value out, to a point computed over every holder's last use.
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
        // Gate 3, the drop site. With nothing handed out it is the binding scope.
        // Otherwise it is the release point the handed-out members' regions already
        // carry, which adds no release the program did not have. The members must
        // agree on one point that lies outside the binding scope and post-dominates
        // it. A point inside means the value never left, so the scope stays the drop
        // site. Any other point refuses.
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
        // Gate 4, containment, tested by structural ancestry rather than by `ord`:
        // every member's allocation site lies in the binding scope's subtree. A
        // cell's site is the scope node and a closure's `Lambda` is an init
        // descendant, so a region from outside refuses the cycle, such as a reused
        // binding identity naming a foreign lambda. The drop site is the scope or
        // post-dominates it, so it inherits the property.
        let contained = members.iter().all(|m| {
            alloc_hir
                .get(m)
                .is_some_and(|&a| pd.in_subtree(a, binding_scope))
        });
        if !contained {
            continue;
        }
        // Gate 5: every tail call in the scope's body, outside nested lambdas, needs
        // a release channel, because a frame-replacing `TailCall` leaves the
        // binding-scope drop as dead code. A member callee rides the stranded-cycle
        // deferral (`stranded_cycle_bindings`). A non-member callee rides
        // `deferred_release_slot`, which needs a resolvable callee to key it at. It
        // also needs no member passed by move (`(g od)`) to a callee that may
        // replace the frame: that activation releases its parameter, which decrefs
        // the arena a second time. A callee `may_replace_frame` rules out is a native,
        // which borrows its arguments and keeps the live binding-scope drop.
        let sites = tail.map(|t| &t.sites);
        let is_member = |b: crate::hir::Binding| -> bool {
            info.binding_source_regions
                .get(&b)
                .is_some_and(|rs| rs.iter().any(|r| scc.contains(r)))
        };
        let strands = sites.is_none_or(|sites| {
            !sites.iter().all(|site| {
                let Some(callee) = site.callee else {
                    return false; // unresolvable → no site to key the deferred release
                };
                if is_member(callee) {
                    return true; // member callee → the stranded-cycle deferral
                }
                // Non-member callee → `deferred_release_slot`.
                !arena.get(callee).may_replace_frame()
                    || !site.arg_bindings.iter().copied().any(is_member)
            })
        });
        if strands {
            continue;
        }
        // Gate 6: a returned member's arena may be released only after the mint that
        // funds the caller's reference. A body whose every tail exit leaves the frame
        // mints inside the body, ahead of every release channel. A body that hands a
        // member out has a drop site that follows the value, from gate 3. A body that
        // does neither keeps the Shared baseline.
        if return_facet && !exits_frame && hands_out.is_empty() {
            continue;
        }
        // Every tail site with a non-member callee takes `deferred_release_slot`; a
        // member callee keeps its own channel. The caller keys these to the root.
        let tail_release_sites: Vec<HirId> = sites
            .map(|sites| {
                sites
                    .iter()
                    .filter(|site| !site.callee.is_some_and(is_member))
                    .map(|site| site.hir_id)
                    .collect()
            })
            .unwrap_or_default();
        // The post-order shadow of gate 4: the binding scope has the highest
        // post-order index in its subtree, so it follows every member's allocation,
        // and a drop site outside it that post-dominates it comes later still. A drop
        // site inside the body fails here in a debug build.
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
        // Root: the SCC closure with the smallest program order, which is distinct
        // per lambda and so deterministic; region ids order nothing. Any member mints
        // the shared physical region at runtime. The root only names the merged slot
        // and carries the single decref, which the caller sets to `drop_site`.
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
    if matches!(hir.kind, HirKind::Lambda(_)) {
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
/// already close the SCC); it matters only for the mixed shape — a
/// self-recursive member a sibling ALSO captures, a size-1 SCC whose retained
/// (sibling-owned) cell the merge collapses via this self-edge
/// (`compute_closure_cycle_merges`).
fn collect_closure_capture_edges(
    hir: &Hir,
    info: &RegionInfo,
    closure_regs: &FxHashSet<Region>,
    succ: &mut FxHashMap<Region, FxHashSet<Region>>,
) {
    if let HirKind::Lambda(lambda) = &hir.kind {
        if let Some(&closure_r) = info.alloc_region.get(&hir.id) {
            for c in &lambda.captures {
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
