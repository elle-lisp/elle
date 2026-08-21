//! Solver-side projection of escape's frontier verdict onto regions.
//!
//! Escape ([`crate::hir::EscapeInfo`]) is the authority for *whether* a value
//! escapes; it answers in its own vocabulary — bindings and `HirId`s — and never
//! sees a region. This module maps that verdict into the region domain using the
//! solver's own `alloc_region` / `binding_source_regions` maps, the coordinate
//! transform the region consumers (the ownership Shared-seed `compute_shared_seeds`,
//! the closure-cycle merge's frontier gate — which reads the return and fiber halves
//! separately, docs/impl/region/letrec.md § The frontier gate — branch
//! compensation's exclusion) need. It is
//! the **only** place escape facts meet regions — escape holds no region plumbing,
//! the solver holds no escape logic.
//!
//! One non-escape reading lives here beside them: the **release route** a binding's
//! binder records ([`Route`]), the analysis-side mirror of the lowerer's
//! `region_to_slot`. Both of its consumers are in this file — the mutated-route
//! refusal `frame_held_regions` applies, and [`value_routed_regions`] — and a mirror
//! that disagreed with itself between the two would poison one release while
//! promising another, so the four binder sites are read exactly once.
//!
//! Projection rule: a frontier **binding** projects through `binding_source_regions`
//! (the regions its value may point into); a frontier **allocation site** (a returned
//! lambda, an atomless aggregate, a call result, an emitted/sent literal) projects
//! through `alloc_region`. A frontier `HirId` that names no allocation (an immediate)
//! is simply absent from `alloc_region` and contributes no region — so escape's
//! deliberate over-approximation (`record_frontier_sites`) costs nothing here.

use rustc_hash::FxHashSet;
use std::collections::HashMap;

use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use crate::hir::region::{Region, RegionInfo};
use crate::hir::EscapeInfo;

/// The region forest's own **reachability** capture-fact: every binding referenced
/// from inside a closure — i.e. every binding that appears in some `Lambda`'s
/// capture set (`CaptureInfo`). This is the region capture-graph at binding
/// granularity, sourced **structurally from the HIR** rather than the lexical
/// proxy `BindingInner::is_captured` the solver is locked out of.
///
/// It is exactly the conservative "is this value also reachable through a closure's
/// env" relation the merge gate's sole-held refusal and branch compensation's
/// value-route taint need — a reachability question, NOT the lifetime/escape one
/// (those read [`EscapeInfo`]). A captured binding is, by construction, in some
/// lambda's capture set, so this set coincides with the structural capture flag
/// while keeping the solver decoupled from the arena proxy (docs/impl/escape.md
/// "Lexical capture is demoted to a structural hint").
pub(super) fn captured_bindings(hir: &Hir) -> FxHashSet<Binding> {
    fn walk(h: &Hir, out: &mut FxHashSet<Binding>) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            for c in captures {
                out.insert(c.binding);
            }
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = FxHashSet::default();
    walk(hir, &mut out);
    out
}

/// Regions a value crosses the **return frontier** through — flowing to a lambda's
/// (or the program's) tail/return, an ownership transfer to the caller. The
/// region-level return facet. Reads `binding_source_regions` and `alloc_region`
/// directly (rather than the whole `RegionInfo`) because the builder-idiom merge
/// seed runs before the rest of `RegionInfo` is final and needs exactly these two
/// maps.
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

/// Regions a value crosses the **fiber frontier** through — emitted, yielded, or
/// sent, so another fiber can reach it. The region-level fiber facet.
///
/// One consumer: the ownership Shared seed (below). A value another fiber can
/// reach cannot be Owned by a bounded activation, whatever counts the crossing
/// takes.
///
/// The frame-held admission reads [`fiber_frontier_site_regions`] instead, not
/// this: a crossing hands the value to a seam that counts a reference of its own,
/// so the second holder it creates is not the uncounted borrow that admission
/// guards against (docs/impl/region/mechanism.md § "A fiber crossing is a counted
/// holder too").
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

/// The **atomless** half of [`fiber_frontier_regions`] alone: a value emitted or
/// sent with no binding to name it (`(yield (%pair …))`, `(chan/send s (%pair …))`),
/// projected through `alloc_region`.
///
/// Split out for the frame-held admission, which judges its bindings one by one and
/// so needs the half no binding answers for.
fn fiber_frontier_site_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    for (&hid, &r) in &info.alloc_region {
        if escape.escapes_fiber_frontier(hid) {
            out.insert(r);
        }
    }
    out
}

/// The ownership **Shared-seed** set: every region a value crosses the
/// activation/fiber frontier through — **return** ∪ **fiber** (emit + send). The
/// *containment* facets (store, capture) are deliberately **not** seeds — they build
/// the Owned subtree and are resolved by external uniqueness, not seeded (a value
/// stored into / captured by a *local* aggregate stays interior and reclaims with
/// it). See `ownership/seeds.rs` / `docs/impl/escape.md`.
pub(crate) fn shared_seed_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out = return_frontier_regions(escape, &info.alloc_region, &info.binding_source_regions);
    out.extend(fiber_frontier_regions(escape, info));
    out
}

/// The regions this frame holds alone for as long as it lives: no holder binding
/// leaves the activation by a **containment** facet, the release route is
/// unmutated, and the region is absent from the fiber frontier's atomless site half.
/// A region with no holder binding at all offers nothing to judge and is refused —
/// except a binding's compiled forward CELL, whose holders are that binding's one
/// indirection out and which therefore carries its verdict, projected alongside the
/// binding's own regions below (region/mechanism.md § "A compiled capture cell is
/// frame-held exactly as its binding is").
///
/// This is the **count** question a *placement* argument cannot answer, and the one
/// admission every mechanism that makes a release fire where none fired before must
/// clear: if the frame is the region's only holder, the new release drops the
/// frame's own reference and nothing else; if it is not, the other holder may be an
/// uncounted borrow in a parked frame, and the release frees a region that frame
/// still resolves through its slot (region/generations.md § "Uncounted-borrow
/// check"). Escape is the sole authority for it (docs/impl/escape.md).
///
/// **The return facet rides along rather than refusing.** Something does read such a
/// region after the frame — the caller, through a reference the tail callee's own
/// `Return` mints, which fires after a release relocated ahead of that call. But the
/// callee reaches a value this frame owns as an **operand** or through its
/// **captured environment** and by no other route, so it either holds a counted (or
/// owning) edge across that gap or cannot name the region at all, in which case its
/// `Return` mints nothing against it (region/mechanism.md § "The callee's return
/// mint, and why the point owes it nothing").
///
/// **Lexical capture is deliberately not a refusal** (region/mechanism.md §
/// "Lexical capture is not a second holder to fear"). A closure's environment does
/// reach what it captures, but that hold is paid for at the moment the env is
/// built: the allocation funnel's cross-region scan increfs a by-value capture's
/// region (balanced by the closure region's free-time cascade), a capture through a
/// cell takes the same count at the cell store, and where the ownership forest
/// admits the containment instead the capture becomes an adopt under which the
/// member's RC is frozen and every decref is a structural no-op. A counted or
/// owning edge is not the uncounted borrow this predicate exists to protect, so
/// the frame's own release still drops the only reference it owns. Capture by a
/// closure that escapes by a **containment** facet is a different matter and is
/// already covered: `binding_escapes_by_containment` folds in escape's capture facet,
/// which propagates such a closure's verdict to every binding it captures. Contrast
/// [`captured_bindings`], the structural graph the *merge* gate reads — merging
/// changes where a value lives, so it needs raw reachability rather than a count.
///
/// **A fiber crossing rides along for the same reason** (region/mechanism.md § "A
/// fiber crossing is a counted holder too"). Each seam that hands a value to another
/// fiber counts a reference of its own before this frame runs on: the park's
/// `EmitEscape` retain going out, the resume value's own mint coming back
/// (`RegionInfo::unfunded_resume_values`), and `chan/send`'s send-site incref. So the
/// admission refuses the containment facets
/// rather than everything beyond return. The fiber frontier's **atomless site half**
/// still refuses: a value emitted or sent with no binding to name it is judged by no
/// holder here at all.
///
/// **A mutated binding is refused for its value ROUTE, so the refusal reaches only
/// as far as that route** (region/mechanism.md § "A mutated holder poisons its
/// value route, not its cell box"). A value-routed release loads one slot and frees
/// the region of whatever it finds there, which a repointed slot makes unanswerable
/// — and that slot belongs to the region's own route binding, never to every
/// binding that names the value ([`mutated_route_regions`]). An env cell's release
/// names the cell BOX instead (`LoadCaptureRaw` + `DecrefCellRegion`), and the box
/// is minted once per activation by `populate_env` and never repointed — an
/// `assign` writes the cell's *content*. So a `cell_release_regions` member keeps
/// its route's mutation and owes only the count argument, which the facets above
/// still ask. The emitter states the same exclusion at its two mutated-slot
/// backstops (`lir::lower::regiondecref::emit_decref_for_region`).
///
/// Two consumers, deliberately sharing one predicate: the branch-arm release window
/// (`region::infer::analyze::decref`) and the frame-exit release the lowerer performs
/// at a tail call (`RegionInfo::frame_held_regions`, region/mechanism.md § "A
/// release past a frame-replacing tail call is not a release").
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
        // The binding's compiled forward CELL rides its binding's verdict. A binding
        // names the closure region its cell points AT, never the cell's own, so a
        // cell region has no holder here and would be refused for want of anything
        // to judge — while its holders are in fact this binding's holders one
        // indirection out: the frame's own slot, plus one counted `closure ⊇ cell`
        // edge per capturer. No route reaches the cell that does not reach the
        // binding (a `DerefCell` read goes THROUGH the cell), so reading it under
        // the same facets and the same mutated-route test asserts nothing new; it
        // names a region the predicate could not see. An ambiguous multi-cell
        // binding yields `None` and stays refused, keeping this in step with the
        // `AdoptCellRegion` emit.
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

/// What a binding's own binder records as the release route for its value — the
/// slot a value-routed release loads (region/mechanism.md § "A release the
/// relocation replicates names a VALUE, and a binder's slot supplies that name").
///
/// **One binding owns a region's route.** The lowerer keys `region_to_slot` on the
/// region's ALLOCATION site (`lir::lower::regionemit::record_region_slot`), so the
/// slot a release loads is the slot of the binding whose *init* allocated the
/// region. Every other binding that names the same value — a cursor an arm walks
/// with, an alias — reaches it through a slot no release ever reads.
///
/// **Four binder sites record a route, and no others.** `Define`, `Let` and
/// `Letrec` are the three [`binder_route`] reads off `binder_init_sites`, which the
/// walk records at exactly those forms; the fourth is the lambda prologue, which
/// records a PARAMETER's slot for the call-result regions its value may name and
/// for no others. So a binding absent from the mirror is read by what introduced it
/// rather than by a blanket verdict either way.
enum Route {
    /// One binder, so one route: the region that binder's init allocated. `None`
    /// where the init allocates nothing — an init that merely names another binding
    /// is absent from `alloc_region`, exactly as it records no slot in the lowerer.
    Binder(Option<Region>),
    /// The lambda prologue's route: a PARAMETER's slot, standing for the
    /// call-result regions the parameter's value may name
    /// (`lir::lower::lambda::body`).
    Prologue(Vec<Region>),
    /// Two different binders, each recording a route of its own, and nothing here
    /// says which one a release loads. A genuine ambiguity rather than a gap in the
    /// mirror.
    Ambiguous,
    /// No site records a slot: a name a PATTERN introduces — a destructuring
    /// binder, a `Match` arm's pattern — or a `Loop` parameter functionalization
    /// minted.
    Unrouted,
}

/// Read [`Route`] for one binding. `regions` is the binding's source-region set,
/// which only the prologue's answer is stated over.
fn binder_route(
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

/// Regions a value-routed release can NAME — the analysis-side reading of the slot
/// `lir::lower::regiondecref::value_release_slot` would load.
///
/// Releasing by region id is the lowerer's default, so this is the set the
/// frame-exit relocation can replicate into a branch arm: only a value route
/// nil-stamps the slot it read, and only a stamped run counts once where a merge's
/// copy and an arm's replica land on one path (region/mechanism.md § "An arm that
/// leaves through a callee takes a replica, not the anchor"). Two halves:
///
/// - **`call_result_regions`** — the class whose release is value-routed
///   unconditionally, whether the slot came from a binder or from the lambda
///   prologue.
/// - **the binder's own route** — a region a `Define`/`Let`/`Letrec` init
///   allocated, whose slot names that value from the binder to the release.
///
/// The binder half is deliberately the conservative reading of the emitter's
/// refusals, because a region claimed here that the emitter then releases by id
/// takes no replica and has lost the per-arm compensation the window displaced. A
/// **captured** binding is refused: its slot holds an env box or a compiled cell
/// rather than the value. A **reassigned fn-local** is refused because its slot is
/// repointed — the backstop for the emitter's own reading of that fact
/// (`lir::lower::emitops::allocate_slot_routed`, which tracks it by slot);
/// functionalization normally versions an in-function reassignment away, leaving
/// the allocating binder a version no `assign` repoints. A region two binders claim
/// keeps the strictest of their verdicts, since `region_to_slot` holds one entry
/// and this cannot say whose.
pub(super) fn value_routed_regions(
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) -> FxHashSet<Region> {
    let mut claimed: FxHashSet<Region> = info.call_result_regions.iter().copied().collect();
    let mut refused: FxHashSet<Region> = FxHashSet::default();
    for &b in binder_init_sites.keys() {
        // The prologue's own regions are call results, so the first half already
        // holds them and this reading needs no source-region set to state them over.
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

/// Regions whose value-routed release would load a slot the program repoints
/// (region/mechanism.md § "A mutated holder poisons its value route, not its cell
/// box").
///
/// The refusal reaches exactly the route [`binder_route`] names, never every
/// binding that holds the value: a cursor an arm walks the value with reaches it
/// through a slot no release reads, so its reassignment cannot make the release
/// name a value the solver did not mean. So a parameter poisons exactly the
/// prologue's own set, while a pattern name and a `Loop` parameter poison nothing
/// at all. The whole-holder reading is left to [`Route::Ambiguous`], where two
/// routes exist and nothing here says which the release loads.
///
/// A mutated binding's compiled forward CELL is refused too: the projection that
/// names the cell asserts exactly what the binding asserts and nothing more
/// (region/mechanism.md § "A compiled capture cell is frame-held exactly as its
/// binding is").
///
/// An env cell is exempt throughout: its release names the BOX at its env index,
/// which `populate_env` mints once per activation and an `assign` never repoints.
fn mutated_route_regions(
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

/// What a frame-replacing tail call's own callee tells the lowerer about the
/// releases and mints around it ([`crate::hir::region::TailCalleeFacts`]).
///
/// Per CALL rather than per region or per function, because it is a claim about
/// *this* callee: how many of the arguments it turns into owned parameters.
///
/// Only a callee this compilation can see qualifies: a `Var` naming a
/// `Let`/`Letrec`/`Define`-bound lambda in this unit. An unresolvable callee is
/// simply absent, and every consumer takes its conservative branch there.
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
        if let HirKind::Lambda {
            params, rest_param, ..
        } = &init.kind
        {
            out.entry(b).or_insert_with(|| LambdaFacts {
                fixed_params: params.len() - usize::from(rest_param.is_some()),
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
/// `functionalize` puts around a read of a `needs_capture` binding — which every
/// mutually-visible top-level `defn` is, so matching the bare `Var` alone would
/// resolve almost no real call.
fn callee_binding(func: &Hir) -> Option<Binding> {
    match &func.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => callee_binding(cell),
        _ => None,
    }
}
