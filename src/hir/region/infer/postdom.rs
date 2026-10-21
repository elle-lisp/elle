// audited: 2026-10-21
//! Structural position over the scope tree: subtree ancestry, loop enclosure,
//! and the post-dominance the single-drop lifetime obligation needs.
//!
//! docs/impl/region/adopt.md
//!
//! The builder-idiom MERGE (`merge::builder`, gate 6) and the ownership-forest
//! ADOPT (`ownership::adopt`) share the obligation. Both route a member's
//! reclamation onto **one** drop point: the parent or root's `DecrefRegion`,
//! fired after that node's whole subtree executes (post-order). That single drop
//! is sound only if it comes **after** the member's last use on every path, with
//! no re-execution of the use afterward.
//!
//! Domination is **structural, never numeric**. A smaller `compute_order` index
//! does *not* imply post-domination once a branch arm or a loop back-edge sits
//! between the use and the drop. This predicate decides it over the scope tree
//! through post-order subtree intervals (`[low[N], order[N]]`, `compute_order`),
//! where structural ancestry and control-flow enclosure are interval tests. The
//! call sites keep an `ord(member) <= ord(root)` comparison only as a
//! `#[cfg(debug_assertions)]` shadow.

use super::*;

/// Which emit mode's obligation a post-dominance check discharges. The two modes
/// differ in exactly one structural fact: whether **containment** pins the
/// member's lifetime to the drop's owner. That fact decides whether the
/// cross-iteration loop guard applies.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EmitMode {
    /// Builder-idiom MERGE: the child is the parent `%pair`'s car/cdr, stored
    /// **only** into it (merge gates 1+4), and its own decref is **suppressed**.
    /// Only the parent reaches it, and only the parent's drop reclaims it. A loop
    /// rebuilding the parent rebuilds the only path to the child, so there is no
    /// cross-iteration re-deref, and the loop-enclosure clause is **waived**. An
    /// in-loop nested literal still merges: that bounded per-iteration arena
    /// reclaim is what the merge exists for.
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
    order: &'a Order,
    ctrl: Vec<Ctrl>,
}

impl<'a> PostDom<'a> {
    pub(super) fn new(hir: &Hir, order: &'a Order) -> Self {
        let mut ctrl = Vec::new();
        collect_ctrl(hir, order, &mut ctrl);
        PostDom { order, ctrl }
    }

    /// This node's post-order execution index.
    pub(super) fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// Is there no `While`/`Loop` enclosing `inner` that does not also enclose
    /// `outer`? Asked of an ancestor `outer` and a descendant `inner`, this is
    /// "does `inner` execute once per execution of `outer`": a loop between the
    /// two re-runs `inner` alone, so the counts diverge. A loop enclosing both is
    /// immaterial — it re-runs the pair together.
    pub(super) fn no_loop_between(&self, inner: HirId, outer: HirId) -> bool {
        let i = self.ord(inner);
        let o = self.ord(outer);
        !self
            .ctrl
            .iter()
            .any(|c| c.is_loop && Self::ctrl_contains(c, i) && !Self::ctrl_contains(c, o))
    }

    /// Is `inner` inside `outer`'s post-order subtree interval, that is, is
    /// `outer` a structural ancestor of `inner` (`compute_order`)?
    pub(super) fn in_subtree(&self, inner: HirId, outer: HirId) -> bool {
        let lo = self.order.low(&outer).unwrap_or(0);
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
    ///   which lives inside that subtree. That is sound under any loop nesting,
    ///   because the use re-derefs only within the drop's own re-executed context.
    /// - **(B) Distinct positions, sequenced and control-flow-clean.** `use_`
    ///   must precede `drop` in post-order. **No** control node may enclose
    ///   exactly one of them: a branch arm or a loop straddling one side breaks
    ///   the straight-line "executes before" that `ord` would otherwise assert.
    ///   For [`EmitMode::Adopt`], **no** `While`/`Loop` may enclose the drop,
    ///   because its free re-runs and would free a member the next iteration
    ///   re-derefs. [`EmitMode::Merge`] waives the loop clause: containment
    ///   (gates 1+4) makes the child reachable only through the parent it is
    ///   rebuilt with.
    ///
    /// Conservative by construction: a shape it cannot prove post-dominated stays
    /// Shared / unmerged (the always-legal baseline), never a use-after-free.
    pub(super) fn drop_post_dominates(&self, drop: HirId, use_: HirId, mode: EmitMode) -> bool {
        if self.in_subtree(use_, drop) {
            return true; // (A)
        }
        // (B) `use_` must sequence strictly before `drop` in post-order. In distinct
        // subtrees (not case A), `ord(use) >= ord(drop)` means `drop` does not even
        // linearize after `use_`: the free precedes the deref.
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
fn collect_ctrl(hir: &Hir, order: &Order, out: &mut Vec<Ctrl>) {
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
            lo: order.low(&hir.id).unwrap_or(0),
            hi: order.get(&hir.id).copied().unwrap_or(0),
            is_loop,
        });
    }
    hir.for_each_child(|c| collect_ctrl(c, order, out));
}
