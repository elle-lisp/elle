//! Which `Emit` sites yield a payload the emitting body owns no reference of
//! (docs/impl/region/owner.md § "Park/unpark symmetry" — "A fiber body owns one
//! reference of every value it yields").
//!
//! A park delivers its payload to the resumer and leaves a copy in `fiber.signal`.
//! Two references answer for that, and they answer to different consumers:
//!
//! - the park's `EmitEscape` retain is the **delivery** reference, consumed by the
//!   resumer's compiler-emitted release of the resume result;
//! - the body's own reference is released by the continuation past the yield —
//!   the release a fiber abandoned while suspended never runs, and the one the
//!   region free's fiber discharge stands in for.
//!
//! A payload the body allocated supplies the second reference itself: its
//! `decref_point` is at or after the `Emit`, in the emitting function. A payload
//! the body merely borrows — a capture, a parameter, a module-level binding —
//! supplies none, so the discharge would release the delivery reference the
//! resumer already consumed. This pass names those sites; `lower_emit` mints the
//! missing reference at each **suspending** one. A terminal emit takes no mint:
//! a halt promotes the fiber to `:dead` and its delivery has no consumer at
//! all, and an error's body-owned reference is reclaimed through the frames'
//! own release tables instead — the raise records its minted delivery
//! (`Fiber::emit_delivery`), so the abandoned-frame walk and the parked frame's
//! discharge run the payload's owed releases with their receipts, where a
//! blanket discharge could not tell a borrowed payload from an owned one.
//!
//! The question is per-**function**, not per-region: a borrowed payload usually
//! does have a `decref_point`, just in the activation that allocated it, whose
//! release runs whatever the fiber does. So each site is compared against the
//! innermost `Lambda` enclosing it, and a payload counts as body-owned only when
//! every region it may live in is released inside that same lambda.
//!
//! Unresolvable is borrowed. Minting where the body already owns a reference
//! strands one per abandoned park — a bounded leak; missing one frees a live
//! value.

use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// The `Emit` sites of `hir` whose payload the emitting body releases nowhere
/// (`RegionInfo::borrowed_emit_payloads`). Runs last in `analyze_regions_with`:
/// it reads the final `region_data` and the merge forest, since a merged child's
/// release is its root's.
pub(super) fn compute_borrowed_emit_payloads(hir: &Hir, info: &RegionInfo) -> FxHashSet<HirId> {
    let mut enclosing: FxHashMap<HirId, Option<HirId>> = FxHashMap::default();
    record_enclosing_lambda(hir, None, &mut enclosing);

    let mut out = FxHashSet::default();
    for (&site, payload) in &info.emit_payload_regions {
        let body = enclosing.get(&site).copied().flatten();
        // An empty payload set is a value the walk resolved to no region at all —
        // an immediate, or a borrow it could not name. Neither leaves the body a
        // reference to release, and the mint is a no-op on an immediate.
        let owned = !payload.is_empty()
            && payload.iter().all(|&r| {
                let root = info.merged_root(r);
                info.region_data
                    .get(&root)
                    .is_some_and(|d| enclosing.get(&d.decref_point).copied().flatten() == body)
            });
        if !owned {
            out.insert(site);
        }
    }
    out
}

/// The `Emit` sites of `hir` whose RESUME value nothing else counts
/// (`RegionInfo::unfunded_resume_values`) — the other direction of the same
/// crossing.
///
/// The resumer pushes the value onto the parked frame's stack and takes no
/// reference for it, so the body reads it through the resumer's own reference
/// unless one is minted here. What already funds a reference is the frame's own
/// return transfer: an `Emit` the frame hands its value back from carries the
/// `Return` marker's mint for the same region, and a second one would strand a
/// reference per resume. So the answer is the emit sites whose result region is off
/// the **return frontier**, read from escape's authoritative verdict rather than a
/// syntactic tail test — a value bound and returned later is as funded as one
/// returned in place.
pub(super) fn compute_unfunded_resume_values(
    hir: &Hir,
    escape: &crate::hir::EscapeInfo,
    info: &RegionInfo,
) -> FxHashSet<HirId> {
    let returned = super::escape::return_frontier_regions(
        escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );
    let mut out = FxHashSet::default();
    collect_emit_sites(hir, &mut out);
    out.retain(|site| {
        info.alloc_region
            .get(site)
            .is_none_or(|&r| !returned.contains(&info.merged_root(r)) && !returned.contains(&r))
    });
    out
}

/// Every `Emit` node id in `hir`.
fn collect_emit_sites(hir: &Hir, out: &mut FxHashSet<HirId>) {
    if matches!(&hir.kind, HirKind::Emit { .. }) {
        out.insert(hir.id);
    }
    hir.for_each_child(|c| collect_emit_sites(c, out));
}

/// Map every node to the innermost `Lambda` enclosing it (`None` at the
/// compilation unit's top level), so "released in the emitting body" is one
/// lookup per region.
fn record_enclosing_lambda(
    hir: &Hir,
    current: Option<HirId>,
    out: &mut FxHashMap<HirId, Option<HirId>>,
) {
    out.insert(hir.id, current);
    let inner = if matches!(&hir.kind, HirKind::Lambda { .. }) {
        Some(hir.id)
    } else {
        current
    };
    hir.for_each_child(|c| record_enclosing_lambda(c, inner, out));
}
