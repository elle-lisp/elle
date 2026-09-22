// audited: 2026-09-22
//! Where a cell's release lands: the env cell's box, and the node a 1-slot
//! container's content drop must cover.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/bindings.md

use super::super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::region::{PinDecref, ProgramOrder};

/// The structural tables the content-drop placement reads, gathered once by the
/// driver: it computes them for the env-cell passes too, and a placement that
/// rebuilt them would walk the tree a second time per unit.
pub(super) struct CellPlacement<'a> {
    pub(super) hir: &'a Hir,
    pub(super) du: &'a DefUseBuilder,
    pub(super) order: &'a HashMap<HirId, u32>,
    pub(super) last_use: &'a HashMap<HirId, HirId>,
    /// Post-order subtree low ends, so containment is an interval test.
    pub(super) subtree_low: &'a HashMap<HirId, u32>,
    pub(super) iter_scopes: &'a [(HirId, u32, u32)],
    pub(super) branches: &'a [super::super::super::arms::ArmSet],
    pub(super) binder_init_sites: &'a HashMap<Binding, Option<HirId>>,
}

/// Give every fn-local 1-slot container the point its content drop is emitted at
/// (docs/impl/region/bindings.md § "Where the content drop lands").
///
/// The cell's own reference to its current content dies at its last access — the
/// latest of its reads and of the node COVERING its writes — and the point is
/// then hoisted until it post-dominates them all.
///
/// A single write is that covering node itself. Several are not: a cell reached
/// from mutually exclusive arms has its latest write inside ONE arm, and a drop
/// there runs on that path alone, leaving every other arm's value held by the
/// cell and released nowhere. The writes' innermost covering node — their lowest
/// common ancestor — is the nearest point after all of them that every storing
/// path reaches.
///
/// A cell CARRIED ACROSS a loop is re-pointed every iteration, so a drop inside
/// the body would free the content the next iteration reads. Such a cell is a
/// loop PARAMETER, i.e. its scope node is the loop itself, so seeding from that
/// node lands the one drop after the loop — where the lowerer emits the loop's
/// own releases. A cell bound INSIDE a loop body has a body scope node instead,
/// so it keeps its seed and drops once per iteration, matching its per-iteration
/// mint. And a loop's parameters stay readable past the loop (the
/// `(while … (assign acc …)) acc` idiom), which is why every step here is a max
/// and not a move.
pub(super) fn place_content_drops(info: &mut RegionInfo, p: &CellPlacement) {
    if info.cell_containers.is_empty() {
        return;
    }
    let ord = |id: HirId| p.order.get(&id).copied().unwrap_or(0);
    let loop_ids: rustc_hash::FxHashSet<HirId> =
        p.iter_scopes.iter().map(|&(id, _, _)| id).collect();
    // Each scope node by the region it introduces, so a binding's scope node is
    // one lookup through `binding_region`.
    let scope_of_region: HashMap<Region, HirId> =
        info.scope_region.iter().map(|(&id, &r)| (r, id)).collect();
    let carried_loop: HashMap<Binding, HirId> = info
        .cell_containers
        .keys()
        .filter_map(|&b| {
            let scope = *info.binding_region.get(&b)?;
            let node = *scope_of_region.get(&scope)?;
            loop_ids.contains(&node).then_some((b, node))
        })
        .collect();
    for (b, c) in info.cell_containers.iter_mut() {
        let store_ords: Vec<u32> = c.stores.sites().map(ord).collect();
        let covering = innermost_covering(p.hir, p.order, p.subtree_low, &store_ords);
        let latest =
            p.du.uses
                .get(b)
                .into_iter()
                .flat_map(|v| v.iter())
                .map(|use_id| p.last_use.get(use_id).copied().unwrap_or(*use_id))
                .chain(covering)
                .chain(carried_loop.get(b).copied())
                .max_by_key(|id| ord(*id));
        if let Some(seed) = latest {
            c.demise = hoist_past_partial_paths(seed, *b, p);
        }
    }
}

/// Move `seed` out to the node of every enclosing branch arm and loop that does
/// not also enclose the BINDER, iterated outward until nothing encloses it.
///
/// The drop must POST-DOMINATE every access: the cell holds one reference
/// whichever path stored it, so a point seeded at the structurally-latest access
/// — which may sit inside one branch arm, or inside a loop the binder is bound
/// outside — would run on that path alone (a leak on every sibling arm) or once
/// per iteration (freeing content a later iteration reads). The lowerer emits a
/// node's releases after it, so a branch node's land after the merge and a loop
/// node's after the loop. An arm or loop the binder is bound INSIDE keeps the
/// seed: the cell itself is per-path or per-iteration there, and so is its drop.
fn hoist_past_partial_paths(seed: HirId, b: Binding, p: &CellPlacement) -> HirId {
    let ord = |id: HirId| p.order.get(&id).copied().unwrap_or(0);
    // The BINDER's position, off the walk's binder-init record — `du.def_site`
    // holds the latest def, and an `assign` is a def, so it would read the very
    // store the hoist is asked about.
    let bdef = p
        .binder_init_sites
        .get(&b)
        .and_then(|s| *s)
        .map(ord)
        .unwrap_or(0);
    let mut demise = seed;
    // One step per enclosing node at most; the bound is a backstop, not a count.
    for _ in 0..p.branches.len() + p.iter_scopes.len() + 1 {
        let o = ord(demise);
        let hoist = p
            .branches
            .iter()
            .filter(|br| {
                br.arms.iter().any(|a| o >= a.lo && o <= a.hi)
                    && !(bdef >= br.node_lo && bdef <= br.node_hi)
            })
            .map(|br| br.id)
            .chain(
                p.iter_scopes
                    .iter()
                    .filter(|&&(_, llo, lhi)| o >= llo && o <= lhi && !(bdef >= llo && bdef <= lhi))
                    .map(|&(id, _, _)| id),
            )
            .filter(|&id| ord(id) > o)
            .max_by_key(|&id| ord(id));
        match hoist {
            Some(id) => demise = id,
            None => break,
        }
    }
    demise
}

/// Collect every branch's arm sets in one walk (`super::super::super::arms` is
/// the shared reading of what counts as an arm). Callers of `branch_arms` walk
/// the tree themselves, each collecting different scopes alongside; the
/// cell-demise hoist needs the arm intervals alone.
pub(super) fn collect_branch_arm_sets(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<super::super::super::arms::ArmSet>,
) {
    out.extend(super::super::super::arms::branch_arms(hir, order, low));
    hir.for_each_child(|c| collect_branch_arm_sets(c, order, low, out));
}

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
