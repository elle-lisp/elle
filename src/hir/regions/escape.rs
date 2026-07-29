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
/// Two consumers: the ownership Shared seed (below), and the branch-arm release
/// window, whose anchor argument is a placement one and therefore says nothing
/// about *other* holders — a value another fiber can reach may be borrowed
/// uncounted by a frame that is parked when the release runs
/// (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
/// the other arms").
pub(crate) fn fiber_frontier_regions(escape: &EscapeInfo, info: &RegionInfo) -> FxHashSet<Region> {
    let mut out: FxHashSet<Region> = FxHashSet::default();
    // Binding half: an emitted / sent binding.
    for (&b, regions) in &info.binding_source_regions {
        if escape.escapes_fiber(b) {
            out.extend(regions.iter().copied());
        }
    }
    // Allocation-site half: an atomless emitted / sent value
    // (`(yield (%pair …))`, `(chan/send s (%pair …))`).
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

/// The two frame-held sets ([`FrameHeld`]), split on whether the **return** facet
/// counts as a refusal: `sole` is the regions whose every holder binding leaves
/// this activation by **no** facet — non-escaping, with no mutated holder unless
/// the region is released through a cell box rather than that holder's slot, and
/// absent from the return/fiber frontiers' atomless site halves — and
/// `return_funded` is the same reading with the return facet alone allowed. A
/// region with no holder binding at all offers nothing to judge and is refused by
/// both.
///
/// This is the **count** question a *placement* argument cannot answer, and the one
/// admission every mechanism that makes a release fire where none fired before must
/// clear: if the frame is the region's only holder, the new release drops the
/// frame's own reference and nothing else; if it is not, the other holder may be an
/// uncounted borrow in a parked frame, and the release frees a region that frame
/// still resolves through its slot (region/generations.md § "Uncounted-borrow
/// check"). Escape is the sole authority for it (docs/impl/escape.md).
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
/// closure that *escapes* is a different matter and is already covered:
/// `binding_escapes_activation` folds in escape's capture facet, which propagates
/// an escaping closure's verdict to every binding it captures. Contrast
/// [`captured_bindings`], the structural graph the *merge* gate reads — merging
/// changes where a value lives, so it needs raw reachability rather than a count.
///
/// **A mutated holder is refused for its value ROUTE, so the refusal reaches only
/// as far as that route** (region/mechanism.md § "A mutated holder poisons its
/// value route, not its cell box"). A value-routed release loads the holder's slot
/// and frees the region of whatever it finds there, which a repointed slot makes
/// unanswerable. An env cell's release names the cell BOX instead
/// (`LoadCaptureRaw` + `DecrefCellRegion`), and the box is minted once per
/// activation by `populate_env` and never repointed — an `assign` writes the
/// cell's *content*. So a `cell_release_regions` member keeps the holder's
/// mutation and owes only the count argument, which the facets above still ask.
/// The emitter states the same exclusion at its two mutated-slot backstops
/// (`lir::lower::regiondecref::emit_decref_for_region`).
///
/// Two consumers, deliberately sharing one predicate: the branch-arm release window
/// (`regions::analyze::decref`) and the frame-exit release the lowerer performs at a
/// tail call (`RegionInfo::sole_frame_held_regions`, region/mechanism.md § "A
/// release past a frame-replacing tail call is not a release").
pub(super) fn sole_frame_held_regions(
    escape: &crate::hir::EscapeInfo,
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
) -> FrameHeld {
    let all = shared_seed_regions(escape, info);
    let fiber_only = fiber_frontier_regions(escape, info);
    let mut held: FxHashSet<Region> = FxHashSet::default();
    let mut refused: FxHashSet<Region> = FxHashSet::default();
    let mut refused_beyond_return: FxHashSet<Region> = FxHashSet::default();
    for (b, regions) in binding_regions {
        let mutated = arena.get(*b).is_mutated;
        let escapes = escape.binding_escapes_activation(*b);
        let escapes_beyond_return = escape.binding_escapes_beyond_return(*b);
        for &r in regions {
            held.insert(r);
            // A MUTATED holder is refused for the reason compensation refuses it as
            // a release route: a slot repointed before the release frees whatever it
            // holds THEN, not what the solver named here. That is a claim about the
            // release, not about the holder, so it is asked per region: an env
            // cell's release names the cell BOX, which `populate_env` mints once per
            // activation and no `assign` repoints, and is untouched by the
            // mutation.
            let route_poisoned = mutated && !info.cell_release_regions.contains(&r);
            if route_poisoned || escapes {
                refused.insert(r);
            }
            if route_poisoned || escapes_beyond_return {
                refused_beyond_return.insert(r);
            }
        }
    }
    FrameHeld {
        sole: held
            .iter()
            .copied()
            .filter(|r| !refused.contains(r) && !all.contains(r))
            .collect(),
        return_funded: held
            .into_iter()
            .filter(|r| !refused_beyond_return.contains(r) && !fiber_only.contains(r))
            .collect(),
    }
}

/// The two frame-held sets, computed together because they differ only in whether
/// the **return** facet counts as a refusal.
pub(super) struct FrameHeld {
    /// No facet at all: the frame holds the region's one reference and nothing
    /// reads it once the frame is gone, so a release needs no funding.
    pub(super) sole: FxHashSet<Region>,
    /// The return facet and no other. Something DOES read the region after the
    /// frame — the caller, through a reference the callee's `Return` mints — so
    /// this set is a precondition, never an admission on its own: its consumer
    /// pairs it with the tail callee's captured-holder edge at each relocation
    /// point, which is the count standing between the frame's release and that
    /// mint (region/mechanism.md § "The callee's return mint, and the edge that
    /// funds the gap"). A superset of `sole`.
    pub(super) return_funded: FxHashSet<Region>,
}

/// What a frame-replacing tail call's own callee tells the lowerer about the
/// releases and mints around it ([`crate::hir::region::TailCalleeFacts`]).
///
/// Both facts are per CALL rather than per region or per function, because both
/// are claims about *this* callee: that it keeps a region alive across its own
/// return mint (which a sibling arm's callee, capturing nothing, does not), and
/// how many of the arguments it turns into owned parameters.
///
/// Only a callee this compilation can see qualifies: a `Var` naming a
/// `Let`/`Letrec`/`Define`-bound lambda in this unit. An unresolvable callee is
/// simply absent, and every consumer takes its conservative branch there.
pub(super) fn tail_callee_facts(
    hir: &Hir,
    info: &RegionInfo,
    frame_replacing_tail_calls: &FxHashSet<HirId>,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
) -> HashMap<HirId, crate::hir::region::TailCalleeFacts> {
    let mut lambda_of: HashMap<Binding, LambdaFacts> = HashMap::new();
    collect_lambda_facts(hir, &mut lambda_of);
    let mut out = HashMap::new();
    collect_call_facts(
        hir,
        info,
        frame_replacing_tail_calls,
        binding_regions,
        &lambda_of,
        &mut out,
    );
    out
}

/// What a lambda-bound binding's lambda contributes at a call to it.
struct LambdaFacts {
    captures: Vec<Binding>,
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
            captures,
            params,
            rest_param,
            ..
        } = &init.kind
        {
            out.entry(b).or_insert_with(|| LambdaFacts {
                captures: captures.iter().map(|c| c.binding).collect(),
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
    info: &RegionInfo,
    frame_replacing_tail_calls: &FxHashSet<HirId>,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
    lambda_of: &HashMap<Binding, LambdaFacts>,
    out: &mut HashMap<HirId, crate::hir::region::TailCalleeFacts>,
) {
    if let HirKind::Call { func, .. } = &h.kind {
        if frame_replacing_tail_calls.contains(&h.id) {
            if let Some(facts) = callee_binding(func).and_then(|b| lambda_of.get(&b)) {
                out.insert(
                    h.id,
                    crate::hir::region::TailCalleeFacts {
                        capture_funded: facts
                            .captures
                            .iter()
                            .flat_map(|c| binding_regions.get(c).into_iter().flatten())
                            .map(|&r| info.merged_root(r))
                            .collect(),
                        fixed_params: facts.fixed_params,
                    },
                );
            }
        }
    }
    h.for_each_child(|c| {
        collect_call_facts(
            c,
            info,
            frame_replacing_tail_calls,
            binding_regions,
            lambda_of,
            out,
        )
    });
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
