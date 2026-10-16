// audited: 2026-10-06
//! Escape's frontier verdicts projected onto regions, and the release-route mirror the frame-held admission reads.
//!
//! docs/impl/escape.md
//! docs/impl/region/held.md
//! docs/impl/region/window.md
//!
//! Escape ([`crate::hir::EscapeInfo`]) answers in bindings and `HirId`s and never
//! sees a region. The functions here map its verdicts into regions through the
//! solver's `binding_source_regions` and `alloc_region`. A frontier binding
//! projects through `binding_source_regions`, the regions its value may point
//! into. A frontier allocation site projects through `alloc_region`: a returned
//! lambda, an atomless aggregate, a call result, or an emitted or sent literal. A
//! frontier `HirId` that allocates nothing is absent from `alloc_region` and adds no
//! region, so escape's over-approximation (`record_frontier_sites`) costs nothing.
//!
//! The release route ([`Route`]) is the analysis-side mirror of the lowerer's
//! `region_to_slot`. [`binder_route`] is its one reading, so the frame-held
//! admission, [`value_routed_regions`] and branch compensation agree on the slot a
//! release loads.

use rustc_hash::FxHashSet;
use std::collections::HashMap;

use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use crate::hir::region::{Region, RegionInfo};
use crate::hir::EscapeInfo;

/// Every binding in some `Lambda`'s capture set, read off the HIR rather than off
/// the arena's lexical `BindingInner::is_captured` flag.
///
/// This is reachability through a closure's environment, not escape: it marks a
/// captured binding whether or not its closure ever leaves. The merge seed, branch
/// compensation and the transfer cut read it. A lifetime question reads
/// [`EscapeInfo`] instead.
pub(super) fn captured_bindings(hir: &Hir) -> FxHashSet<Binding> {
    fn walk(h: &Hir, out: &mut FxHashSet<Binding>) {
        if let HirKind::Lambda(lambda) = &h.kind {
            for c in &lambda.captures {
                out.insert(c.binding);
            }
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = FxHashSet::default();
    walk(hir, &mut out);
    out
}

/// Regions a value crosses the **return frontier** through: it flows to a
/// lambda's or the program's tail or `Return`, which hands it to the caller.
pub(crate) fn return_frontier_regions(
    escape: &EscapeInfo,
    alloc_region: &HashMap<HirId, Region>,
    binding_source_regions: &HashMap<Binding, Vec<Region>>,
) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    // Binding half: a heap-valued binding flowing to a tail/return.
    for (&b, regions) in binding_source_regions {
        if escape.binding_escapes_via_return(b) {
            out.extend(regions.iter().copied());
        }
    }
    // Allocation-site half: a returned lambda, or an atomless aggregate / call
    // result reached at a tail with no binding to hold it.
    for (&hid, &r) in alloc_region {
        if escape.escapes_return_frontier(hid) {
            out.insert(r);
        }
    }
    out
}

/// Regions a value crosses the **fiber frontier** through: it is emitted, yielded
/// or sent, so another fiber can reach it.
///
/// The ownership Shared seed and the closure-cycle merge's fiber gate read it. The
/// frame-held admission reads only [`fiber_frontier_site_regions`], because each
/// fiber seam counts a reference of its own for a binding's value.
pub(crate) fn fiber_frontier_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    // Binding half: an emitted / sent binding.
    for (&b, regions) in &info.binding_source_regions {
        if escape.escapes_fiber(b) {
            out.extend(regions.iter().copied());
        }
    }
    out.extend(fiber_frontier_site_regions(escape, info));
    out
}

/// The **atomless** half of [`fiber_frontier_regions`]: a value emitted or sent
/// with no binding to name it (`(yield (%pair …))`, `(chan/send s (%pair …))`),
/// projected through `alloc_region`.
///
/// The frame-held admission judges its bindings one by one, so it needs this half
/// separately: no binding answers for it.
fn fiber_frontier_site_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    for (&hid, &r) in &info.alloc_region {
        if escape.escapes_fiber_frontier(hid) {
            out.insert(r);
        }
    }
    out
}

/// The ownership **Shared-seed** set (`ownership::seeds`): the **return** frontier
/// joined with the **fiber** frontier. The containment facets, store and capture,
/// seed nothing. A value stored into or captured by a local aggregate stays inside
/// it, and external uniqueness decides that case.
pub(crate) fn shared_seed_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out = return_frontier_regions(escape, &info.alloc_region, &info.binding_source_regions);
    out.extend(fiber_frontier_regions(escape, info));
    out
}

/// The regions this frame holds alone for as long as it lives. Every mechanism
/// that makes a release fire on a path where none fired before must clear this
/// count admission.
///
/// A region is admitted when no holder binding escapes by a containment facet
/// (`binding_escapes_by_containment`), its release route is unmutated
/// ([`mutated_route_regions`]), and it is absent from the fiber frontier's atomless
/// site half. The return facet, a lexical capture and a fiber crossing each leave a
/// counted or owning holder, so none of them refuses. A region with no holder
/// binding is refused, except a binding's compiled forward cell, which takes its
/// binding's verdict.
///
/// Two mechanisms read the one predicate: the branch-arm release window
/// (`analyze::decref::branch`) and the lowerer's frame-exit relocation at a tail
/// call, through `RegionInfo::frame_held_regions`.
pub(super) fn frame_held_regions(
    escape: &crate::hir::EscapeInfo,
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) -> FxHashSet<Region> {
    let fiber = fiber_frontier_site_regions(escape, info);
    let poisoned = mutated_route_regions(arena, info, binding_regions, binder_init_sites);
    let mut held: FxHashSet<Region> = FxHashSet::default();
    let mut refused: FxHashSet<Region> = FxHashSet::default();
    for (b, regions) in binding_regions {
        let escapes_by_containment = escape.binding_escapes_by_containment(*b);
        // A binding names the closure region its compiled forward cell points at,
        // never the cell's own, so the cell has no holder binding of its own. Its
        // holders are this binding's, one indirection out, so it takes the
        // binding's verdict. A binding with several cells yields `None` and stays
        // refused, as the `AdoptCellRegion` emit does.
        let cell = info.single_cell_region_of(*b);
        for &r in regions.iter().chain(cell.iter()) {
            held.insert(r);
            if poisoned.contains(&r) || escapes_by_containment {
                refused.insert(r);
            }
        }
    }
    held.retain(|r| !refused.contains(r) && !fiber.contains(r));
    held
}

/// The slot a value-routed release loads for a binding's value, as the binding's
/// own binder records it.
///
/// The lowerer keys `region_to_slot` on a region's allocation site
/// (`record_region_slot`), so one binding owns a region's route: the binding whose
/// init allocated it. Every other binding that names the value, such as a cursor
/// or an alias, holds a slot no release reads. Four binder sites record a route.
/// `Define`, `Let` and `Letrec` record theirs in `binder_init_sites`; the lambda
/// prologue records a parameter's slot.
pub(super) enum Route {
    /// One binder, so one route: the region that binder's init allocated. `None`
    /// where the init allocates nothing: an init that only names another binding
    /// is absent from `alloc_region`, and the lowerer records no slot for it.
    Binder(Option<Region>),
    /// The lambda prologue's route: a parameter's slot, standing for the
    /// call-result regions the parameter's value may name
    /// (`lir::lower::lambda::body`).
    Prologue(Vec<Region>),
    /// Two different binders each record a route, and nothing here says which one
    /// a release loads.
    Ambiguous,
    /// No site records a slot: a name a pattern introduces (a destructuring
    /// binder, a `Match` arm's pattern), or a `Loop` parameter that
    /// functionalization minted.
    Unrouted,
}

/// Read [`Route`] for one binding. Only the prologue's answer reads `regions`, the
/// binding's source-region set.
pub(super) fn binder_route(
    b: Binding,
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    regions: &[Region],
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) -> Route {
    match binder_init_sites.get(&b) {
        Some(&Some(init)) => Route::Binder(info.alloc_region.get(&init).copied()),
        Some(None) => Route::Ambiguous,
        None if arena.get(b).scope == crate::hir::arena::BindingScope::Parameter => {
            Route::Prologue(
                regions
                    .iter()
                    .copied()
                    .filter(|r| info.call_result_regions.contains(r))
                    .collect(),
            )
        }
        None => Route::Unrouted,
    }
}

/// Regions a value-routed release can name: the analysis-side reading of the slot
/// `lir::lower::regiondecref::value_release_slot` would load.
///
/// Only a value-routed release nil-stamps the slot it read, so only its region can
/// take a replica when the frame-exit relocation copies a release into a branch
/// arm. Two halves claim a region:
///
/// - **`call_result_regions`**: their release is value-routed whether the slot came
///   from a binder or from the lambda prologue.
/// - **the binder's own route**: a region a `Define`, `Let` or `Letrec` init
///   allocated, whose slot names that value from the binder to the release.
///
/// The binder half copies the emitter's refusals, because a region claimed here
/// that the emitter then releases by id gets no replica. A captured binder is
/// refused: its slot holds an env box or a compiled cell rather than the value. A
/// reassigned fn-local binder is refused because its slot is repointed, which
/// `lir::lower::emitops::allocate_slot_routed` tracks by slot. A region that two
/// binders claim keeps the stricter verdict, since `region_to_slot` holds one entry.
pub(super) fn value_routed_regions(
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) -> FxHashSet<Region> {
    let mut claimed: FxHashSet<Region> = info.call_result_regions.iter().copied().collect();
    let mut refused: FxHashSet<Region> = FxHashSet::default();
    for &b in binder_init_sites.keys() {
        // The prologue's regions are call results, which the first half already
        // holds, so this reading passes no source-region set.
        let Route::Binder(Some(r)) = binder_route(b, arena, info, &[], binder_init_sites) else {
            continue;
        };
        if arena.get(b).needs_capture() || info.reassigned_local_bindings.contains(&b) {
            refused.insert(r);
        } else {
            claimed.insert(r);
        }
    }
    claimed.retain(|r| !refused.contains(r));
    claimed
}

/// Regions whose value-routed release would load a slot the program repoints.
///
/// The refusal reaches exactly the route [`binder_route`] names, never every
/// binding that holds the value. A parameter poisons the prologue's own set, and a
/// pattern name or a `Loop` parameter poisons nothing. [`Route::Ambiguous`] poisons
/// every region the binding names. A mutated binding's compiled forward cell is
/// refused with its binding.
///
/// An env cell (`cell_release_regions`) is exempt: its release names the box at
/// its env index, which `populate_env` mints once per activation and an `assign`
/// never repoints.
pub(super) fn mutated_route_regions(
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    for (&b, regions) in binding_regions {
        if !arena.get(b).is_mutated {
            continue;
        }
        out.extend(info.single_cell_region_of(b));
        match binder_route(b, arena, info, regions, binder_init_sites) {
            Route::Binder(r) => out.extend(r),
            Route::Prologue(rs) => out.extend(rs),
            Route::Ambiguous => out.extend(regions.iter().copied()),
            Route::Unrouted => {}
        }
    }
    out.retain(|r| !info.cell_release_regions.contains(r));
    out
}

/// What each frame-replacing tail call's own callee tells the lowerer
/// ([`crate::hir::region::TailCalleeFacts`]), keyed by the call.
///
/// Only a callee this compilation can see qualifies: a `Var` naming a lambda that
/// a `Let`, `Letrec` or `Define` binds in this unit. An unresolvable callee is
/// absent, and the lowerer takes its conservative branch there.
pub(super) fn tail_callee_facts(
    hir: &Hir,
    frame_replacing_tail_calls: &FxHashSet<HirId>,
) -> HashMap<HirId, crate::hir::region::TailCalleeFacts> {
    let mut lambda_of: HashMap<Binding, LambdaFacts> = HashMap::new();
    collect_lambda_facts(hir, &mut lambda_of);
    let mut out = HashMap::new();
    collect_call_facts(hir, frame_replacing_tail_calls, &lambda_of, &mut out);
    out
}

/// What a lambda-bound binding's lambda contributes at a call to it.
struct LambdaFacts {
    /// Parameters that take one argument each — `params` less the rest parameter,
    /// which collects the overflow into a fresh list instead.
    fixed_params: usize,
}

/// Binding → the facts of the lambda it is bound to. A binding bound to anything
/// but a lambda contributes nothing; a binding bound twice keeps the first, since
/// an ambiguous callee is one this pass must not claim to resolve.
fn collect_lambda_facts(h: &Hir, out: &mut HashMap<Binding, LambdaFacts>) {
    let mut record = |b: Binding, init: &Hir| {
        if let Some(lambda) = init.kind.as_lambda() {
            out.entry(b).or_insert_with(|| LambdaFacts {
                fixed_params: lambda.params.len() - usize::from(lambda.rest_param.is_some()),
            });
        }
    };
    match &h.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                record(*b, init);
            }
        }
        HirKind::Define { binding, value, .. } => record(*binding, value),
        _ => {}
    }
    h.for_each_child(|c| collect_lambda_facts(c, out));
}

fn collect_call_facts(
    h: &Hir,
    frame_replacing_tail_calls: &FxHashSet<HirId>,
    lambda_of: &HashMap<Binding, LambdaFacts>,
    out: &mut HashMap<HirId, crate::hir::region::TailCalleeFacts>,
) {
    if let HirKind::Call { func, .. } = &h.kind {
        if frame_replacing_tail_calls.contains(&h.id) {
            if let Some(facts) = callee_binding(func).and_then(|b| lambda_of.get(&b)) {
                out.insert(
                    h.id,
                    crate::hir::region::TailCalleeFacts {
                        fixed_params: facts.fixed_params,
                    },
                );
            }
        }
    }
    h.for_each_child(|c| collect_call_facts(c, frame_replacing_tail_calls, lambda_of, out));
}

/// The binding a callee position names, looking through the `DerefCell` wrapper
/// `functionalize` puts around a read of a `needs_capture` binding. A top-level
/// `defn` that another function calls is such a binding, so matching the bare
/// `Var` alone would miss most calls.
fn callee_binding(func: &Hir) -> Option<Binding> {
    match &func.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => callee_binding(cell),
        _ => None,
    }
}
