//! Structural post-dominance for the single-drop lifetime obligation shared by
//! the builder-idiom MERGE (`merge.rs` gate 6) and the ownership-forest ADOPT
//! (`ownership/adopt.rs`). Both route a member's reclamation onto **one** drop
//! point — the parent/root's `DecrefRegion`, fired at its `decref_point` after
//! that node's whole subtree executes (post-order). For that single drop to be
//! sound it must come **after** the member's last use on every path, with no
//! re-execution of the use afterward (region/adopt.md § "The lifetime obligation
//! the root carries").
//!
//! Domination is **structural, never numeric**: a smaller `compute_order` index
//! does *not* imply post-domination once a branch arm or a loop back-edge sits
//! between the use and the drop. This predicate decides it over the scope tree via
//! post-order subtree intervals (`[low[N], order[N]]`, `compute_subtree_low`) —
//! structural ancestry and control-flow enclosure are interval tests — not by
//! comparing indices. The `ord(member) <= ord(root)` comparison the two call sites
//! used to gate on now survives only as their `#[cfg(debug_assertions)]` shadow.

use super::*;

/// Which emit mode's obligation a post-dominance check discharges. The two modes
/// differ in exactly one structural fact — whether **containment** pins the
/// member's lifetime to the drop's owner — which decides whether the
/// cross-iteration loop guard applies.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EmitMode {
    /// Builder-idiom MERGE: the child is the parent `%pair`'s car/cdr, stored
    /// **only** into it (merge gates 1+4), and its own decref is **suppressed** —
    /// it is reachable solely through the parent and reclaimed only by the
    /// parent's drop. A loop rebuilding the parent rebuilds the only path to the
    /// child, so there is no cross-iteration re-deref: the loop-enclosure clause
    /// is **waived** (an in-loop nested literal still merges — the
    /// bounded-per-iteration arena reclaim the merge exists for).
    Merge,
    /// Ownership-forest ADOPT: a store-adopted member keeps its own
    /// `DecrefValueRegion`, an independent reclamation touch a loop re-runs after
    /// the root's free — the cross-iteration UAF. The loop-enclosure clause
    /// **applies**.
    Adopt,
}

/// A control-flow node's post-order subtree interval, with whether it is
/// iterative. `If`/`Cond`/`Match`/`And`/`Or` separate two positions into
/// distinct arms / short-circuit operands; `While`/`Loop` additionally re-run
/// their body.
struct Ctrl {
    lo: u32,
    hi: u32,
    is_loop: bool,
}

/// Post-order subtree intervals plus the control-flow-node table for one
/// compilation unit, built once and reused for every member check (the call
/// sites loop over many members per subtree / per pair site).
pub(super) struct PostDom<'a> {
    order: &'a HashMap<HirId, u32>,
    low: HashMap<HirId, u32>,
    ctrl: Vec<Ctrl>,
}

impl<'a> PostDom<'a> {
    pub(super) fn new(hir: &Hir, order: &'a HashMap<HirId, u32>) -> Self {
        let low = compute_subtree_low(hir, order);
        let mut ctrl = Vec::new();
        collect_ctrl(hir, order, &low, &mut ctrl);
        PostDom { order, low, ctrl }
    }

    fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// Is `inner` inside `outer`'s post-order subtree interval — i.e. is `outer`
    /// a structural ancestor of `inner` (`compute_subtree_low`)?
    pub(super) fn in_subtree(&self, inner: HirId, outer: HirId) -> bool {
        let lo = self.low.get(&outer).copied().unwrap_or(0);
        let oi = self.ord(inner);
        oi >= lo && oi <= self.ord(outer)
    }

    fn ctrl_contains(c: &Ctrl, oi: u32) -> bool {
        oi >= c.lo && oi <= c.hi
    }

    /// Does the drop's free — fired after `drop`'s whole subtree executes — come
    /// after `use_`'s last deref on every path, with no re-execution of `use_`
    /// afterward? The sound structural test:
    ///
    /// - **(A) `drop` is a structural ancestor of `use_`.** Its `emit_decrefs_for`
    ///   fires after its whole subtree, hence after every execution of `use_`,
    ///   which lives inside that subtree — sound under any loop nesting (the use
    ///   re-derefs only within the drop's own re-executed context).
    /// - **(B) Distinct positions, sequenced and control-flow-clean.** `use_`
    ///   must precede `drop` in post-order, **no** control node may enclose
    ///   exactly one of them (a branch arm / a loop straddling one side breaks the
    ///   straight-line "executes before" that `ord` would otherwise assert), and —
    ///   for [`EmitMode::Adopt`] — **no** `While`/`Loop` may enclose the drop (its
    ///   free re-runs and would free a member the next iteration re-derefs; the
    ///   cross-iteration UAF). [`EmitMode::Merge`] waives the loop clause:
    ///   containment (gates 1+4) makes the child reachable only through the
    ///   parent it is rebuilt with.
    ///
    /// Conservative by construction: a shape it cannot prove post-dominated stays
    /// Shared / unmerged (the always-legal baseline), never a use-after-free.
    pub(super) fn drop_post_dominates(&self, drop: HirId, use_: HirId, mode: EmitMode) -> bool {
        if self.in_subtree(use_, drop) {
            return true; // (A)
        }
        // (B) `use_` must sequence strictly before `drop` in post-order. Distinct
        // subtrees (not Case A) with `ord(use) >= ord(drop)` means `drop` does not
        // even linearize after `use_` — the free precedes the deref.
        if self.ord(use_) >= self.ord(drop) {
            return false;
        }
        let u = self.ord(use_);
        let d = self.ord(drop);
        for c in &self.ctrl {
            let u_in = Self::ctrl_contains(c, u);
            let d_in = Self::ctrl_contains(c, d);
            if u_in != d_in {
                return false; // a control node encloses exactly one — they are separated
            }
            if c.is_loop && d_in && mode == EmitMode::Adopt {
                return false; // a loop re-runs the free after a member deref
            }
        }
        true
    }
}

/// Collect every control-flow node (`If`/`Cond`/`Match`/`While`/`Loop`/`And`/`Or`)
/// with its post-order subtree interval and whether it is iterative.
fn collect_ctrl(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<Ctrl>,
) {
    let is_loop = matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. });
    let is_ctrl = is_loop
        || matches!(
            &hir.kind,
            HirKind::If { .. }
                | HirKind::Cond { .. }
                | HirKind::Match { .. }
                | HirKind::And(_)
                | HirKind::Or(_)
        );
    if is_ctrl {
        out.push(Ctrl {
            lo: low.get(&hir.id).copied().unwrap_or(0),
            hi: order.get(&hir.id).copied().unwrap_or(0),
            is_loop,
        });
    }
    hir.for_each_child(|c| collect_ctrl(c, order, low, out));
}
