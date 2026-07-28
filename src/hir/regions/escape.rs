//! Solver-side projection of escape's frontier verdict onto regions.
//!
//! Escape ([`crate::hir::EscapeInfo`]) is the authority for *whether* a value
//! escapes; it answers in its own vocabulary — bindings and `HirId`s — and never
//! sees a region. This module maps that verdict into the region domain using the
//! solver's own `alloc_region` / `binding_source_regions` maps, the coordinate
//! transform the region consumers (the ownership Shared-seed `compute_shared_seeds`,
//! the merge gate's not-returned check, branch compensation's exclusion) need. It is
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

/// The regions whose every holder binding leaves this activation by **no** facet:
/// non-mutated, uncaptured, non-escaping, and absent from the return/fiber
/// frontiers' atomless site halves. A region with no holder binding at all offers
/// nothing to judge and is refused too.
///
/// This is the **count** question a *placement* argument cannot answer, and the one
/// admission every mechanism that makes a release fire where none fired before must
/// clear: if the frame is the region's only holder, the new release drops the
/// frame's own reference and nothing else; if it is not, the other holder may be an
/// uncounted borrow in a parked frame, and the release frees a region that frame
/// still resolves through its slot (region/generations.md § "Uncounted-borrow
/// check"). Escape is the sole authority for it (docs/impl/escape.md).
///
/// Two consumers, deliberately sharing one predicate: the branch-arm release window
/// (`regions::analyze::decref`) and the frame-exit release the lowerer performs at a
/// tail call (`RegionInfo::sole_frame_held_regions`, region/mechanism.md § "A
/// release past a frame-replacing tail call is not a release").
pub(super) fn sole_frame_held_regions(
    hir: &Hir,
    escape: &crate::hir::EscapeInfo,
    arena: &crate::hir::arena::BindingArena,
    info: &RegionInfo,
    binding_regions: &std::collections::HashMap<Binding, Vec<Region>>,
) -> FxHashSet<Region> {
    let frontier = shared_seed_regions(escape, info);
    let captured = captured_bindings(hir);
    let mut held: FxHashSet<Region> = FxHashSet::default();
    let mut refused: FxHashSet<Region> = FxHashSet::default();
    for (b, regions) in binding_regions {
        // A MUTATED or CAPTURED holder is refused for the reason compensation
        // refuses it as a release route: a slot repointed before the release frees
        // whatever it holds THEN, and a captured value is reachable through the
        // closure env — including the env of the very callee a tail call installs.
        let unsafe_holder = arena.get(*b).is_mutated
            || captured.contains(b)
            || escape.binding_escapes_activation(*b);
        for &r in regions {
            held.insert(r);
            if unsafe_holder {
                refused.insert(r);
            }
        }
    }
    held.into_iter()
        .filter(|r| !refused.contains(r) && !frontier.contains(r))
        .collect()
}
