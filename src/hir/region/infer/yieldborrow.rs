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
//! missing reference at each **suspending** one — an error emit leaves through the
//! unwind path and a halt promotes the fiber to `:dead`, so no instruction past
//! either ever runs and neither reaches the discharge (both are terminal).
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
