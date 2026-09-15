// audited: 2026-09-14
//! The releases a `break` jumps over, re-anchored onto the block it leaves.
//!
//! docs/impl/region/anchors.md

use super::super::super::*;

/// Re-anchor every release a `break` jumps over onto the block it leaves.
///
/// The pin above covers the value the break CARRIES. Every other region whose
/// release sits in the same window — inside the block's body, at or after the
/// break site, before the exit label — is passed over by the identical jump, and
/// has no consumer to be handed to: the release is emitted into unreachable code
/// and the region is held to fiber teardown. Re-anchoring to `last_use[block]` —
/// the first point both the break path and the fall-through path reach, the same
/// anchor the broken value takes — is enough, and needs no release at the break
/// site: moving a release LATER can only over-keep (docs/impl/region/mechanism.md
/// § "A release the break jumps over is not a release").
///
/// The window is read off the structural order: a node's releases are skipped
/// exactly when its post-order index is at or above the break's. That covers the
/// break node itself (`lower_break` terminates with the jump, so its own decrefs
/// land in the dead block after it) and every enclosing `let`/`begin`, whose
/// releases the lowerer emits after the body.
///
/// Two scopes bound the window, both because a release inside them must run a
/// different number of times than the block's exit label is reached: a nested
/// `While`/`Loop` (a loop-body value is re-allocated per iteration, so one
/// release cannot cover N) and a nested `Lambda` (its releases run in another
/// activation, against another frame's slots). Inside either, the release stays
/// where it is — still skipped on the break path, an over-keep bounded by one
/// iteration / one call, never a mis-free.
///
/// A third condition guards the anchor itself: the exit label has to be a point
/// every path actually **reaches**. A frame-replacing exit inside the body — a
/// `Return`, or a `Call` in tail position, which the lowerer emits as `TailCall`
/// — leaves through the callee instead of arriving there, so a release moved to
/// the anchor would be dead on exactly the path that used to run it (trading one
/// leak for another). Such a block declines the window whole.
pub(super) fn pin_break_skipped_releases(
    info: &mut RegionInfo,
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    last_use: &HashMap<HirId, HirId>,
    break_skip_blocks: &[(HirId, Vec<HirId>)],
) {
    if break_skip_blocks.is_empty() {
        return;
    }
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let low = compute_subtree_low(hir, order);
    let mut scopes = BreakWindowScopes::default();
    collect_window_scopes(hir, order, &low, &mut scopes);
    // Where each region is allocated, as post-order indices — what the loop
    // barrier below is read off. A region absent here is allocated by a caller
    // (a parameter), so no loop in this unit re-allocates it. Capture cells
    // count as allocated at their `Begin`, which is where `populate_env` mints
    // them.
    let mut alloc_sites: HashMap<Region, Vec<u32>> = HashMap::new();
    for (alloc_id, &region) in &info.alloc_region {
        alloc_sites.entry(region).or_default().push(ord(*alloc_id));
    }
    for (begin_id, cells) in &info.begin_cell_regions {
        for &(_b, region) in cells {
            alloc_sites.entry(region).or_default().push(ord(*begin_id));
        }
    }

    for (block_id, sites) in break_skip_blocks {
        // The window opens at the EARLIEST targeting break: a release after it is
        // skipped whenever that break fires, and the pin has to hold for every
        // path through the block, not just the last one.
        let Some(first_break) = sites.iter().map(|s| ord(*s)).min() else {
            continue;
        };
        let block_lo = low.get(block_id).copied().unwrap_or(0);
        let block_hi = ord(*block_id);
        let anchor = last_use.get(block_id).copied().unwrap_or(*block_id);
        let anchor_ord = ord(anchor);
        // Only the barriers nested INSIDE this block matter: one enclosing the
        // block encloses the anchor too, so it constrains nothing here.
        let inner_loops: Vec<(u32, u32)> = scopes
            .loops
            .iter()
            .copied()
            .filter(|&(lo, hi)| block_lo <= lo && hi < block_hi)
            .collect();
        // A nested lambda is two things at once: a barrier for the releases
        // inside it, and the frame boundary that says whose exits a
        // `frame_exits` entry belongs to.
        let inner_lambdas: Vec<(u32, u32)> = scopes
            .lambdas
            .iter()
            .copied()
            .filter(|&(lo, hi)| block_lo <= lo && hi < block_hi)
            .collect();
        // A frame exit inside a targeting break's own VALUE does not refuse the
        // window: that break already jumps over every release in it, so on that
        // path there is nothing left for the exit to strand
        // (docs/impl/region/mechanism.md § "A release the break jumps over is not
        // a release", third boundary). Only an exit on a path that would
        // otherwise ARRIVE at the release — the block's fall-through — does.
        let break_values: Vec<(u32, u32)> = sites
            .iter()
            .map(|s| (low.get(s).copied().unwrap_or(0), ord(*s)))
            .collect();
        if scopes.frame_exits.iter().any(|&e| {
            e >= first_break
                && e <= block_hi
                && !inner_lambdas.iter().any(|&(lo, hi)| lo <= e && e <= hi)
                && !break_values.iter().any(|&(lo, hi)| lo <= e && e <= hi)
        }) {
            continue;
        }
        for (region, d) in info.region_data.iter_mut() {
            let dord = ord(d.decref_point);
            // In the body (the block node's own decrefs are already at the
            // anchor), at or after the break, and not already later than it.
            if dord < first_break || dord < block_lo || dord >= block_hi || dord >= anchor_ord {
                continue;
            }
            // A lambda's body releases against its own frame's slots, so the
            // enclosing block's exit label is not a point that activation
            // reaches — a release there never hoists, whatever it names.
            if inner_lambdas
                .iter()
                .any(|&(lo, hi)| lo <= dord && dord <= hi)
            {
                continue;
            }
            // A loop barrier is about the COUNT: it holds back a region the loop
            // body allocates, which is re-allocated per iteration and so needs
            // its release per iteration. A region allocated outside — a
            // parameter above all, which has no allocation site here at all — is
            // one per activation, and one release at the anchor covers it
            // exactly once.
            if inner_loops.iter().any(|&(lo, hi)| {
                lo <= dord
                    && dord <= hi
                    && alloc_sites
                        .get(region)
                        .is_some_and(|sites| sites.iter().any(|&a| lo <= a && a <= hi))
            }) {
                continue;
            }
            d.decref_point = anchor;
        }
    }
}

/// The structural facts `pin_break_skipped_releases` reads off one compilation
/// unit, all as post-order indices so containment is an interval test.
#[derive(Default)]
struct BreakWindowScopes {
    /// Subtree intervals of the iterative scopes (`While`/`Loop`). A release
    /// inside one is held back only for a region that scope ALLOCATES, which is
    /// re-allocated per iteration and so needs its release per iteration; a
    /// region the loop merely reads is one per activation and hoists.
    loops: Vec<(u32, u32)>,
    /// Subtree intervals of the `Lambda`s: both an unconditional barrier (the
    /// body runs in its own activation, against its own frame's slots) and the
    /// frame boundary that says whose exits a `frame_exits` entry belongs to.
    lambdas: Vec<(u32, u32)>,
    /// Nodes that leave the enclosing frame instead of falling through to it: a
    /// `Call` in tail position, lowered as a frame-replacing `TailCall`. One in a
    /// block's window means the block's exit label is not a point every path
    /// reaches.
    ///
    /// A `Return` node is deliberately NOT one (docs/impl/region/mechanism.md
    /// § "A release the break jumps over is not a release", third boundary):
    /// `lower_return` emits the return mint and no control flow, so control falls
    /// through it to the exit label like any other value.
    frame_exits: Vec<u32>,
}

/// Collect [`BreakWindowScopes`] over the whole tree in one walk.
fn collect_window_scopes(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut BreakWindowScopes,
) {
    let lo = low.get(&hir.id).copied().unwrap_or(0);
    let hi = order.get(&hir.id).copied().unwrap_or(0);
    match &hir.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => out.loops.push((lo, hi)),
        HirKind::Lambda { .. } => out.lambdas.push((lo, hi)),
        HirKind::Call { is_tail: true, .. } => out.frame_exits.push(hi),
        _ => {}
    }
    hir.for_each_child(|c| collect_window_scopes(c, order, low, out));
}
