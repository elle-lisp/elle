// audited: 2026-11-25
//! The structural facts the branch-arm window reads off one compilation unit,
//! all as post-order indices so containment is an interval test.
//!
//! docs/impl/region/window.md

use super::super::super::arms::{branch_arms, ArmSet};
use super::super::super::*;

/// Region → the post-order index of every site that mints it: the branch-arm
/// window's live-in anchors (docs/impl/region/window.md).
///
/// An allocation is born at its node, an owned or rest parameter's placeholder at
/// its `Lambda`, a compiled capture cell at the scope that mints it, and a
/// collection a pattern builds at its `Destructure` or `Match`. A region with no
/// entry here is never live-in, so the window leaves its release where it is.
///
/// No holder binding anchors. The slot a value-routed release loads belongs to
/// the region's route: the binder whose init allocated it, which contains that
/// allocation, or the parameter whose slot the prologue records. Any other
/// binding an arm introduces records no slot for the region, so the region the
/// branch received is still the branch's to release.
pub(super) fn birth_sites(
    hir: &Hir,
    info: &RegionInfo,
    binding_regions: &HashMap<Binding, Vec<Region>>,
    order: &Order,
) -> HashMap<Region, Vec<u32>> {
    fn parameters(
        h: &Hir,
        binding_regions: &HashMap<Binding, Vec<Region>>,
        out: &mut Vec<(Region, HirId)>,
    ) {
        if let HirKind::Lambda(lambda) = &h.kind {
            for p in lambda.params.iter().chain(lambda.rest_param.iter()) {
                out.extend(
                    binding_regions
                        .get(p)
                        .into_iter()
                        .flatten()
                        .map(|&r| (r, h.id)),
                );
            }
        }
        h.for_each_child(|c| parameters(c, binding_regions, out));
    }
    let mut sites: Vec<(Region, HirId)> = Vec::new();
    parameters(hir, binding_regions, &mut sites);
    sites.extend(info.alloc_region.iter().map(|(&site, &r)| (r, site)));
    for (&scope, cells) in &info.begin_cell_regions {
        sites.extend(cells.iter().map(|&(_, r)| (r, scope)));
    }
    for (&node, built) in &info.pattern_rest_regions {
        sites.extend(built.iter().map(|c| (c.region, node)));
    }
    let mut out: HashMap<Region, Vec<u32>> = HashMap::new();
    for (r, site) in sites {
        out.entry(r)
            .or_default()
            .push(order.get(&site).copied().unwrap_or(0));
    }
    out
}

/// The structural facts [`super::branch::pin_branch_arm_releases`] reads off one
/// compilation unit, all as post-order indices so containment is an interval
/// test.
#[derive(Default)]
pub(super) struct BranchWindowScopes {
    /// Every branch, with its own and its arms' subtree intervals
    /// ([`crate::hir::region::infer::arms`]). An `If` is a two-armed branch.
    /// Every premise here is stated over one arm and its siblings, never over the
    /// branch's kind or arity. A short-circuiting form contributes the arms of its
    /// nested-`If` equivalent, one entry per conditional level. All the entries
    /// of one form name that form's node, so they share the anchor and the
    /// live-in premise. The first to fire moves the release to the anchor, and
    /// the rest then find it at or past that point.
    pub(super) branches: Vec<ArmSet>,
    /// Subtree intervals of the scopes a release may not be hoisted OUT of. An
    /// iterative scope (`While`/`Loop`) re-allocates its body per iteration. A
    /// `Lambda` runs its body in its own activation, against its own frame's
    /// slots.
    pub(super) barriers: Vec<(u32, u32)>,
    /// Subtree intervals of the `Lambda`s alone — the frame boundary, which says
    /// whose exits a `frame_exits` entry belongs to.
    pub(super) lambdas: Vec<(u32, u32)>,
    /// The tail calls that may replace the frame. One inside a branch means its
    /// merge label is not a point every arm reaches, so the branch anchors only the
    /// releases the relocation can replicate into that arm.
    ///
    /// Narrower than the break window's own frame-exit set, which counts every
    /// `Return` and every tail `Call`. A functionalized `Return` inside an arm
    /// stores the branch's result and jumps to the merge rather than leaving, and
    /// a tail call to a *native* falls through to it. Only a callee that can
    /// replace the frame actually skips the merge. Reading the coarser set would
    /// narrow every native-tail dispatch arm for nothing.
    pub(super) frame_exits: Vec<FrameExit>,
}

/// One frame-replacing tail call, as the branch-arm window reads it.
pub(super) struct FrameExit {
    /// The call, which the window records against an anchored callee region.
    pub(super) call: HirId,
    /// Post-order index of the call, so containment in a branch or in a nested
    /// lambda is an interval test.
    pub(super) at: u32,
    /// The regions the call names as its **callee**: the closure region it reaches
    /// the callee through. This is the half of `TailExitHoist::exempt` no
    /// owned-parameter release stands in for. The relocation replicates no such
    /// release. The deferred callee channel runs it on the closure path, through
    /// the demise at the call or the window's record (`anchored_callee_calls`;
    /// docs/impl/region/relocate.md).
    pub(super) callee: rustc_hash::FxHashSet<Region>,
}

/// Collect [`BranchWindowScopes`] over the whole tree in one walk.
pub(super) fn collect_branch_scopes(
    hir: &Hir,
    info: &RegionInfo,
    order: &Order,
    frame_replacing_tail_calls: &rustc_hash::FxHashSet<HirId>,
    out: &mut BranchWindowScopes,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let lo = |id: HirId| order.low(&id).unwrap_or(0);
    match &hir.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => {
            out.barriers.push((lo(hir.id), ord(hir.id)))
        }
        HirKind::Lambda { .. } => {
            out.barriers.push((lo(hir.id), ord(hir.id)));
            out.lambdas.push((lo(hir.id), ord(hir.id)));
        }
        HirKind::Call { func, .. } if frame_replacing_tail_calls.contains(&hir.id) => {
            // The callee half of `TailExitHoist::exempt`, read off the same
            // `operand_value_regions` the lowerer reads so the two cannot disagree
            // about which releases the relocation will decline to replicate.
            let mut callee = rustc_hash::FxHashSet::default();
            info.operand_value_regions(func, &mut callee);
            out.frame_exits.push(FrameExit {
                call: hir.id,
                at: ord(hir.id),
                callee,
            });
        }
        _ => {}
    }
    out.branches.extend(branch_arms(hir, order));
    hir.for_each_child(|c| collect_branch_scopes(c, info, order, frame_replacing_tail_calls, out));
}
