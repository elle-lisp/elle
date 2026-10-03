// audited: 2026-09-28
//! Branch compensation's pins, one submodule per subject, and the arm and release
//! helpers they share.
//!
//! docs/impl/region/compensate.md
//! docs/impl/region/window.md
//!
//! No path may leave a branch without releasing a region that was live-in to it.
//! The submodules pin the mechanisms that discharge that obligation:
//!
//! - `window` — the branch-arm release window, and the frame-exit relocation that
//!   narrows it.
//! - `positions` — the arms of `cond`, `and` and `or`.
//! - `mutated` — the reassignment that refuses a release route, and the bindings
//!   that own one.
//! - `routes` — the per-arm `head` and `tail` compensation routes.
//! - `envcell` — the env cell's compensating release.

use super::*;
use crate::value::SymbolId;

mod envcell;
mod mutated;
mod positions;
mod routes;
mod window;

/// The body HirIds of the first `Match` in the tree, in arm order.
fn first_match_arms(hir: &Hir) -> Option<Vec<HirId>> {
    if let HirKind::Match { arms, .. } = &hir.kind {
        return Some(arms.iter().map(|(_p, _g, body)| body.id).collect());
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_match_arms(c);
        }
    });
    found
}

/// Does `arm` carry a compensating release for any region the binding named
/// `name` may point into?
fn arm_compensates(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    name: &str,
    arm: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_compensation
        .get(&arm)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// Does every region the binding named `name` may point into carry its release
/// OUTSIDE `arms` — i.e. at a node every arm reaches?
///
/// This is the branch-arm release window's signature (docs/impl/region/window.md): the
/// region's one `decref_point` is re-anchored onto the branch, so no arm holds it and
/// none needs a compensating one.
fn release_clears_the_arms(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    name: &str,
    arms: &[HirId],
) -> bool {
    let b = find_binding_by_name(hir, name, arena)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    regions
        .iter()
        .all(|&r| region_release_clears_the_arms(hir, info, r, arms))
}

/// Every binder of `name` that records a release route, as `(binding, region)` —
/// the region its `Let`/`Letrec`/`Define` INIT allocated, which is what
/// `region_to_slot` is keyed on (docs/impl/region/replicate.md).
/// More than one entry where functionalization split the name into versions, each
/// with an allocating init of its own.
fn binder_routes(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    name: &str,
) -> Vec<(Binding, Region)> {
    fn walk(
        h: &Hir,
        name: &str,
        arena: &BindingArena,
        info: &RegionInfo,
        out: &mut Vec<(Binding, Region)>,
    ) {
        let mut record = |b: &Binding, init: &Hir| {
            if arena.get(*b).name == SymbolId::of(name) {
                out.extend(info.alloc_region.get(&init.id).map(|&r| (*b, r)));
            }
        };
        match &h.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, init) in bindings {
                    record(b, init);
                }
            }
            HirKind::Define { binding, value, .. } => record(binding, value),
            _ => {}
        }
        h.for_each_child(|c| walk(c, name, arena, info, out));
    }
    let mut out = Vec::new();
    walk(hir, name, arena, info, &mut out);
    assert!(!out.is_empty(), "`{name}` has no allocating binder");
    out
}

/// [`release_clears_the_arms`] for ONE region, so a pin can name the region it
/// means rather than every region its holder may point into — an env-celled
/// binding holds a box placeholder beside the value's own region, and the two
/// answer to different release routes.
fn region_release_clears_the_arms(hir: &Hir, info: &RegionInfo, r: Region, arms: &[HirId]) -> bool {
    let order = compute_order(hir);
    let low = compute_subtree_low(hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    match info.region_data.get(&r) {
        Some(d) => {
            let o = ord(d.decref_point);
            !arms
                .iter()
                .any(|&a| low.get(&a).copied().unwrap_or(0) <= o && o <= ord(a))
        }
        None => false,
    }
}

/// Does `node` carry a per-arm (`tail`) release for a region the binding named
/// `name` may point into?
fn arm_decrefs(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    name: &str,
    node: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_arm_decrefs
        .get(&node)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// The HirIds of every `Return` node inside `[lo, hi]` of the post-order index.
fn returns_within(hir: &Hir, order: &HashMap<HirId, u32>, lo: u32, hi: u32) -> Vec<HirId> {
    find_all(hir, |h| matches!(&h.kind, HirKind::Return { .. }))
        .into_iter()
        .filter(|id| {
            let o = order.get(id).copied().unwrap_or(0);
            o >= lo && o <= hi
        })
        .collect()
}
