// audited: 2026-10-21
//! The structural facts the branch-arm window reads off one compilation unit,
//! all as post-order indices so containment is an interval test.
//!
//! docs/impl/region/window.md

use super::super::super::arms::{branch_arms, ArmSet};
use super::super::super::*;

/// Every binding whose initializer merely NAMES another binding — an alias.
///
/// Such a binder allocates nothing, so `record_region_slot` records no slot for
/// it (`alloc_region` has no entry at a `Var` init). No value-routed release can
/// ever load its slot. It is therefore not an anchor for the branch-arm window's
/// live-in premise. An arm that introduces one has not given birth to the value,
/// and the region the branch received is still the branch's to release
/// (docs/impl/region/window.md).
///
/// The descent looks through `DerefCell`, the wrapper functionalization puts
/// around a read of a `needs_capture` binding, for the same reason the tail
/// callee's resolution does: matching the bare `Var` alone would miss most real
/// aliases.
///
/// One alias binder does own a slot, and the allocation half of the anchor set is
/// what keeps it honest. A whole-value read of a 1-slot container mints a
/// placeholder region AT the read (`counted_cell_read_sites`), which is an
/// `alloc_region` entry at this very init. So the reader's own placeholder still
/// anchors on the reader. The container's region, which the reader only names,
/// does not.
pub(super) fn alias_binders(hir: &Hir) -> rustc_hash::FxHashSet<Binding> {
    fn names_a_binding(init: &Hir) -> bool {
        match &init.kind {
            HirKind::Var(_) => true,
            HirKind::DerefCell { cell } => names_a_binding(cell),
            _ => false,
        }
    }
    fn walk(h: &Hir, out: &mut rustc_hash::FxHashSet<Binding>) {
        match &h.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, init) in bindings {
                    if names_a_binding(init) {
                        out.insert(*b);
                    }
                }
            }
            HirKind::Define { binding, value, .. } if names_a_binding(value) => {
                out.insert(*binding);
            }
            _ => {}
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = rustc_hash::FxHashSet::default();
    walk(hir, &mut out);
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
    /// Post-order index of the call, so containment in a branch or in a nested
    /// lambda is an interval test.
    pub(super) at: u32,
    /// The regions the call names as its **callee**: the closure region it reaches
    /// the callee through. This is the half of `TailExitHoist::exempt` no
    /// owned-parameter release stands in for. The relocation leaves such a
    /// release where it sits, and the deferred callee channel runs it from there
    /// (`deferred_release_slot`; docs/impl/region/relocate.md).
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
                at: ord(hir.id),
                callee,
            });
        }
        _ => {}
    }
    out.branches.extend(branch_arms(hir, order));
    hir.for_each_child(|c| collect_branch_scopes(c, info, order, frame_replacing_tail_calls, out));
}
