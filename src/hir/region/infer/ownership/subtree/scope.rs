// audited: 2026-10-20
//! The innermost structural scope enclosing a set of HIR nodes: where a co-owned group or an activation's SCC is freed.
//!
//! docs/impl/region/ownership.md

use super::super::super::*;
use rustc_hash::FxHashSet;

/// The innermost structural scope node (`Let`/`Letrec`/`Begin`/`Loop`/`Block`) that
/// lexically encloses every HirId in `targets`, or `None` if they share no common
/// structural scope within one function body. This is the co-owned-cycle cut's drop
/// point ([`compute_owned_region_groups`](super::compute_owned_region_groups)): the
/// lowerer's `emit_decrefs_for` on this node fires after its whole body, so a
/// `FreeRegionGroup` emitted there post-dominates every member's last use and every
/// pass-through-alias release inside the scope.
///
/// Computed as the deepest node common to every target's structural-scope ancestor stack
/// (the longest common prefix). A `Lambda` **resets** the stack — its body is a separate
/// function, so a scope outside the lambda cannot enclose a node inside it; a group whose
/// members live directly in a lambda body with no enclosing structural scope returns
/// `None` and stays Shared (the always-legal baseline). The scope node's own id is not in
/// its descendants' stacks (recorded at entry, before the node is pushed), which is what
/// makes the result a strict *ancestor* — and thus a post-dominator in post-order.
pub(in crate::hir::region::infer) fn innermost_enclosing_scope(
    hir: &Hir,
    targets: &FxHashSet<HirId>,
) -> Option<HirId> {
    fn is_scope(k: &HirKind) -> bool {
        matches!(
            k,
            HirKind::Let { .. }
                | HirKind::Letrec { .. }
                | HirKind::Begin(_)
                | HirKind::Loop { .. }
                | HirKind::Block { .. }
        )
    }
    fn walk(
        h: &Hir,
        targets: &FxHashSet<HirId>,
        stack: &mut Vec<HirId>,
        found: &mut Vec<Vec<HirId>>,
    ) {
        if targets.contains(&h.id) {
            found.push(stack.clone());
        }
        if matches!(h.kind, HirKind::Lambda { .. }) {
            // Separate function body — a fresh ancestor stack (outer scopes do not enclose
            // it). All members of one co-owned cycle are allocated in one activation, so
            // a cross-lambda common prefix would be empty (→ refuse), which is correct.
            let mut inner = Vec::new();
            h.for_each_child(|c| walk(c, targets, &mut inner, found));
            return;
        }
        let pushed = is_scope(&h.kind);
        if pushed {
            stack.push(h.id);
        }
        h.for_each_child(|c| walk(c, targets, stack, found));
        if pushed {
            stack.pop();
        }
    }
    let mut found: Vec<Vec<HirId>> = Vec::new();
    walk(hir, targets, &mut Vec::new(), &mut found);
    // Every target must have been located, or the result would be a scope enclosing only
    // some of them — refuse.
    if found.len() != targets.len() {
        return None;
    }
    let first = found.first()?;
    let mut common = first.len();
    for s in &found[1..] {
        let mut i = 0;
        while i < common && i < s.len() && s[i] == first[i] {
            i += 1;
        }
        common = i;
    }
    (common > 0).then(|| first[common - 1])
}
