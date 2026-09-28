// audited: 2026-09-28
//! Which `Emit` sites yield a payload the emitting body owns no reference of.
//!
//! docs/impl/region/park.md
//!
//! The rule is park.md's "A fiber body owns one reference of every value it
//! yields". What this pass decides is which sites fail it, and `lower_emit`
//! mints the missing reference at each **suspending** one.
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

/// The `Emit` sites of `hir` whose RESUME value this body must mint a reference
/// for (`RegionInfo::unfunded_resume_values`) — the other direction of the same
/// crossing.
///
/// The resumer pushes the value onto the parked frame's stack and takes no
/// reference for it, so the body reads it through the resumer's own reference
/// unless one is minted here. The mint pairs with the release of the binding that
/// names the `Emit`, so the answer is the emit sites a binder names. A returned
/// value changes nothing: the binding is released all the same, and the `Return`
/// marker mints the caller's reference on top. An unnamed `Emit` is a returning
/// position's own tail, which no slot releases, so it takes no mint.
pub(super) fn compute_unfunded_resume_values(hir: &Hir) -> FxHashSet<HirId> {
    let mut out = FxHashSet::default();
    collect_bound_emit_sites(hir, &mut out);
    out
}

/// Every `Emit` node id in `hir` that is the init of a binder whose slot is its
/// release route — a `let`, `letrec` or `def`. A `Loop` parameter records no
/// route, so it is left out.
fn collect_bound_emit_sites(hir: &Hir, out: &mut FxHashSet<HirId>) {
    let mut name = |init: &Hir| {
        if matches!(&init.kind, HirKind::Emit { .. }) {
            out.insert(init.id);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (_, init) in bindings {
                name(init);
            }
        }
        HirKind::Define { value, .. } => name(value),
        _ => {}
    }
    hir.for_each_child(|c| collect_bound_emit_sites(c, out));
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
