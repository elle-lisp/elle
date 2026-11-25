// audited: 2026-11-24
//! The release a `break`'s carried value takes past its block, and the second, per-pass release of one a loop made.
//!
//! docs/impl/region/anchors.md
//! docs/impl/region/breakloop.md

use super::super::super::*;
use super::cells::collect_iter_scopes;
use crate::hir::region::{PinDecref, ProgramOrder};

/// Give each region a loop makes and a `break` carries out its pass scope, the
/// post-order interval of the loop's body. From here on a pin inside the scope
/// moves the region's `decref_point`, and a pin outside it moves `exit_point`
/// (`PinDecref`). Runs before any pin, so every pin is read by where it lands.
///
/// A region qualifies under the four conditions of docs/impl/region/breakloop.md.
/// The innermost loop around every allocation site of the region lies inside a
/// block whose `break_sites` carry it, and one of that block's breaks lies in
/// the loop's body. The value route and the frame-held admission hold, and no
/// other mechanism releases the region. A `Loop` parameter is the mutated
/// binder of the source after functionalization: it hands its value to the
/// next pass, so a region one holds keeps its one release. A fold back to one
/// release answers each later stage that could break the pair
/// (`settle_exit_points`, `drop_merged_exit_points`).
pub(super) fn open_pass_scopes(
    info: &mut RegionInfo,
    hir: &Hir,
    order: &Order,
    break_sites: &[(HirId, Vec<Region>)],
    break_skip_blocks: &[(HirId, Vec<HirId>)],
    binding_regions: &HashMap<Binding, Vec<Region>>,
) {
    if break_sites.is_empty() {
        return;
    }
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let mut loops: Vec<(HirId, u32, u32)> = Vec::new();
    collect_iter_scopes(hir, order, &mut loops);
    if loops.is_empty() {
        return;
    }
    let mut carried_on: rustc_hash::FxHashSet<Region> = rustc_hash::FxHashSet::default();
    collect_loop_parameter_regions(hir, binding_regions, &mut carried_on);
    let mut alloc_sites: HashMap<Region, Vec<(HirId, u32)>> = HashMap::new();
    for (&site, &r) in &info.alloc_region {
        alloc_sites.entry(r).or_default().push((site, ord(site)));
    }
    let breaks_of: HashMap<HirId, &Vec<HirId>> =
        break_skip_blocks.iter().map(|(b, s)| (*b, s)).collect();
    for (block_id, regions) in break_sites {
        let Some(sites) = breaks_of.get(block_id) else {
            continue;
        };
        let block_lo = order.low(block_id).unwrap_or(0);
        let block_hi = ord(*block_id);
        for &r in regions {
            if !qualifies(info, r) || carried_on.contains(&r) {
                continue;
            }
            let Some(allocs) = alloc_sites.get(&r) else {
                continue;
            };
            // The innermost loop inside the block whose BODY holds every
            // allocation site: the smallest such interval.
            let body = loops
                .iter()
                .filter(|&&(_, lo, hi)| block_lo <= lo && hi < block_hi)
                .filter(|&&(_, lo, hi)| allocs.iter().all(|&(_, a)| lo <= a && a < hi))
                .min_by_key(|&&(_, lo, hi)| hi - lo);
            let Some(&(_, lo, hi)) = body else {
                continue;
            };
            if !sites.iter().any(|&s| lo <= ord(s) && ord(s) < hi) {
                continue;
            }
            let (seed, _) = allocs[0];
            info.region_data
                .entry(r)
                .or_insert(RegionData::at(seed))
                .pass_scope = Some((lo, hi));
        }
    }
}

/// The regions every `Loop` parameter of the tree holds.
fn collect_loop_parameter_regions(
    hir: &Hir,
    binding_regions: &HashMap<Binding, Vec<Region>>,
    out: &mut rustc_hash::FxHashSet<Region>,
) {
    if let HirKind::Loop { bindings, .. } = &hir.kind {
        for (b, _) in bindings {
            out.extend(binding_regions.get(b).into_iter().flatten().copied());
        }
    }
    hir.for_each_child(|c| collect_loop_parameter_regions(c, binding_regions, out));
}

/// The admission half of the four conditions: a release a value route can
/// name, a region this frame holds alone, and no other mechanism's release.
fn qualifies(info: &RegionInfo, r: Region) -> bool {
    info.value_routed_regions.contains(&r)
        && info.frame_held_regions.contains(&r)
        && !info.suppressed_decref_regions.contains(&r)
        && !info.owned_group_members.contains(&r)
        && !info.cell_release_regions.contains(&r)
        && !info.mutated_binding_value_regions.contains(&r)
        && info.merged_root(r) == r
}

/// Extend each BROKEN value's regions to where the `Block` it was handed to is
/// CONSUMED. A `Break` is the dual of a consuming node: it does not use its
/// operand, it TRANSFERS it. The value becomes the block's value, and dies
/// wherever the block's value dies. Two facts make the pin necessary:
///
///  - `break` lowers to a jump to the block's exit label. So a release anchored
///    anywhere inside the body is emitted into the break's unreachable
///    fall-through and never runs. The value is then held to fiber teardown
///    (the `break-value*` probes in tests/impl/probe/ measure it).
///  - the block's own exit label is not late enough on its own. The block's
///    value may flow straight into a consumer (`(f (block … (break v)))`), and
///    releasing at the exit would free it under that consumer.
///
/// `last_use[block]` is exactly "the node that consumes the block's value" (its
/// own id when nothing does), and the lowerer emits a node's decrefs after it.
/// For the block itself that is after the exit label. A binding that names the
/// block's value extends further through the ordinary binding chain, and every
/// extension is a max, so the latest wins (tests/impl/region-break-transfer.lisp).
/// The borrow a break carries takes the same pin: the block hands it on with the
/// value (docs/impl/region/borrow.md). For a region with a pass scope this pin
/// lands outside the scope, so it sets the exit release.
pub(super) fn pin_break_transfers(
    info: &mut RegionInfo,
    last_use: &LastUse,
    sites: &[(HirId, Vec<Region>)],
    porder: ProgramOrder<'_>,
) {
    for (block_id, regions) in sites {
        let lu = last_use.get(block_id).copied().unwrap_or(*block_id);
        info.region_data
            .pin_all_to(regions.iter().copied(), lu, porder);
    }
}

/// Close every pass scope, and fold a region back to one release where its pass
/// release cannot stand.
///
/// The pass release must lie in the loop's body, or one release covers every
/// pass again. It must also lie outside every `break` of the block. A point
/// inside a break's own value runs on the breaking pass before the jump, and
/// frees the value the block hands on. A point at the break lands in the dead
/// block after it. A region with no pin outside its scope has nothing past the
/// block to release, and keeps its one release.
pub(super) fn settle_exit_points(
    info: &mut RegionInfo,
    order: &Order,
    break_skip_blocks: &[(HirId, Vec<HirId>)],
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let breaks: Vec<(u32, u32)> = break_skip_blocks
        .iter()
        .flat_map(|(_, sites)| sites)
        .map(|s| (order.low(s).unwrap_or(0), ord(*s)))
        .collect();
    for d in info.region_data.values_mut() {
        let Some((lo, hi)) = d.pass_scope.take() else {
            continue;
        };
        let Some(exit) = d.exit_point else {
            continue;
        };
        let pass = ord(d.decref_point);
        let in_body = lo <= pass && pass < hi;
        let in_break = breaks.iter().any(|&(blo, bhi)| blo <= pass && pass <= bhi);
        if !in_body || in_break {
            d.decref_point = exit;
            d.exit_point = None;
        }
    }
}

/// Fold a merge child's exit release back into its one release. A child's own
/// release is its root's, so nothing stamps the child's slot on a pass that
/// loops on, and an exit release by that slot would free a value the root's
/// release already freed (docs/impl/region/breakloop.md). Runs after both
/// merge seeds.
pub(in crate::hir::region::infer) fn drop_merged_exit_points(info: &mut RegionInfo) {
    let children: Vec<Region> = info
        .region_data
        .iter()
        .filter(|(&r, d)| d.exit_point.is_some() && info.merged_root(r) != r)
        .map(|(&r, _)| r)
        .collect();
    for r in children {
        let d = info.region_data.get_mut(&r).expect("collected above");
        d.decref_point = d.exit_point.take().expect("collected above");
    }
}
