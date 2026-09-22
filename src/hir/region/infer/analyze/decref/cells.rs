// audited: 2026-09-21
//! Where a cell's release lands: the env cell's box, and the node a 1-slot
//! container's content drop must cover.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/bindings.md

use super::super::super::*;
use crate::hir::region::{PinDecref, ProgramOrder};

/// Clamp each env cell's box release to at-or-after every release routed THROUGH
/// that cell (docs/impl/region/cells.md § "A cell's release lands at or after
/// every release routed through that cell").
///
/// A captured binding's init value and the box that holds it are addressed by one
/// env index. The value's `DecrefValueRegion` loads the box RAW and lets
/// `result_region_of` unwrap it to the content, so it READS the box's page; the
/// box's `DecrefCellRegion` frees that page. Emitted in the other order, the
/// unwrap reads memory the free reclaimed.
///
/// At one `decref_point` the release order already states this — a value release
/// that unwraps a cell reads deepest and sorts ahead of the cell release that
/// frees the page (`lir::lower::Lowerer::order_releases`,
/// docs/impl/region/rules.md Rule 4). Across two points nothing does, and the two
/// points diverge by construction: both regions
/// ride the binding's uses through the binding-chain extension, then the VALUE
/// region takes a second, later bound from its allocation site's last use, which
/// follows the binder's own value out to whatever consumes it. The cell region is
/// a phantom placeholder with no allocation site, so it keeps the binding-use
/// bound alone and the box is freed at the capture.
///
/// The one region a value route reads through the cell is the region the
/// binding's own binder ALLOCATED — the entry `record_region_slot` makes against
/// the binder's slot, which for an env-celled binding is the env index
/// (`lir::lower::binding::define`). A region the binding merely NAMES records no
/// slot and takes the release-by-id route, which reads no page: `(def @c n)`
/// names its parameter's phantom region and allocates nothing, so its box owes
/// that region's release nothing. A binder whose init allocates nothing, a
/// binding two binders claim, and a captured-reassigned binding (whose init
/// reference is dropped off its own register rather than through the cell) are
/// all absent from the route for the same reason, and are left alone here.
///
/// The clamp is a maximum like every other pin, so it can only move the box
/// release later; the point it produces is then taken out of any enclosing loop,
/// because an env cell is minted once per activation and its release must fire
/// once per activation whatever drew it inward.
pub(super) fn pin_cell_release_after_routed_releases(
    info: &mut RegionInfo,
    order: &HashMap<HirId, u32>,
    iter_scopes: &[(HirId, u32, u32)],
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
) {
    if info.cell_release_regions.is_empty() {
        return;
    }
    let porder = ProgramOrder::new(order);
    // Staged: the scan reads one region's `decref_point` while the pin writes
    // another's, which one pass over `region_data` cannot borrow.
    let mut pins: Vec<(Region, HirId)> = Vec::new();
    for (b, regions) in inference_binding_regions {
        let cells: Vec<Region> = regions
            .iter()
            .copied()
            .filter(|r| info.cell_release_regions.contains(r))
            .collect();
        if cells.is_empty() || info.captured_reassigned_bindings.contains(b) {
            continue;
        }
        let Some(&Some(init)) = binder_init_sites.get(b) else {
            continue;
        };
        let Some(routed) = info.alloc_region.get(&init).copied() else {
            continue;
        };
        let Some(at) = info.region_data.get(&routed).map(|d| d.decref_point) else {
            continue;
        };
        let at = post_loop_placement(at, iter_scopes, order).unwrap_or(at);
        for c in cells {
            pins.push((c, at));
        }
    }
    for (cell, at) in pins {
        info.region_data.pin_to(cell, at, porder);
    }
}

/// The node an env cell's release takes instead of `at`, so that it fires once
/// per activation rather than once per iteration: the OUTERMOST `While`/`Loop`
/// enclosing `at`, which the lowerer emits after the loop. `None` when `at` is in
/// no loop, or is already at or past the enclosing loop's own node — a release
/// anchored at the loop node already runs once per execution of the loop.
///
/// An ancestor carries the largest post-order index, so the outermost enclosing
/// scope is the containing interval with the greatest high end.
pub(super) fn post_loop_placement(
    at: HirId,
    iter_scopes: &[(HirId, u32, u32)],
    order: &HashMap<HirId, u32>,
) -> Option<HirId> {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let at_ord = ord(at);
    iter_scopes
        .iter()
        .filter(|&&(_, lo, hi)| lo <= at_ord && at_ord <= hi)
        .max_by_key(|&&(_, _, hi)| hi)
        .map(|&(id, _, _)| id)
        .filter(|&id| ord(id) > at_ord)
}
/// The innermost node whose post-order subtree covers every one of `sites` — the
/// sites' lowest common ancestor.
///
/// A 1-slot container's content drop must run once on every path any store ran
/// on, and the stores' LCA is the nearest node that qualifies: it contains them
/// all, so a post-order index puts it after each, and it is a single node rather
/// than one position per arm (docs/impl/region/bindings.md § "Where the content
/// drop lands"). One store answers itself, which is the placement a straight-line
/// cell already had.
///
/// The LCA is inside the same lambda the stores are, which the cell's own scope
/// node is not: a `(var u nil)` at the head of a function body has the `Lambda`
/// for a scope node, and the lowerer runs that node's releases in the ENCLOSING
/// function, where the cell has no slot at all.
pub(super) fn innermost_covering(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    sites: &[u32],
) -> Option<HirId> {
    let lo = low.get(&hir.id).copied().unwrap_or(0);
    let hi = order.get(&hir.id).copied().unwrap_or(0);
    if !sites.iter().all(|&s| lo <= s && s <= hi) {
        return None;
    }
    let mut inner = None;
    hir.for_each_child(|c| {
        if inner.is_none() {
            inner = innermost_covering(c, order, low, sites);
        }
    });
    inner.or(Some(hir.id))
}

/// Collect every iterative-scope node (`While` or `Loop` — `while` lowers to
/// either) with its post-order subtree interval `[low, order]`, so containment
/// of a HirId is an interval test (`low <= ord(x) <= order`; see
/// `compute_subtree_low`). Used by the env-cell release hoist to find the
/// outermost loop enclosing a cell-release region's `decref_point`.
pub(super) fn collect_iter_scopes(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<(HirId, u32, u32)>,
) {
    if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
        let lo = low.get(&hir.id).copied().unwrap_or(0);
        let hi = order.get(&hir.id).copied().unwrap_or(0);
        out.push((hir.id, lo, hi));
    }
    hir.for_each_child(|c| collect_iter_scopes(c, order, low, out));
}
