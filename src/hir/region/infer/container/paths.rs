// audited: 2026-11-10
//! The paths on which a form hands up a sub-expression's value as its own.
//!
//! docs/impl/region/reads.md

use crate::hir::expr::{Hir, HirKind};

/// Visit each sub-expression whose value `h` hands up as its own, one per path:
/// a statement wrapper's tail, and every arm of a branch. Any other form
/// produces its value itself and visits nothing.
pub(super) fn for_each_path<'h>(h: &'h Hir, mut visit: impl FnMut(&'h Hir)) {
    match &h.kind {
        // A statement wrapper selects a value exactly as a branch arm does: the
        // walk returns the LAST expression's regions and nothing else, so the
        // tail is the one path there is.
        HirKind::Begin(exprs) => {
            if let Some(tail) = exprs.last() {
                visit(tail);
            }
        }
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            visit(then_branch);
            visit(else_branch);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, body) in clauses {
                visit(body);
            }
            if let Some(e) = else_branch {
                visit(e);
            }
        }
        // An unmatched value signals rather than falling through to a value,
        // so a `Match`'s arms are every value-producing path it has. An arm's
        // pattern names are irrelevant: what is asked of the arm is what its
        // BODY produces.
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                visit(body);
            }
        }
        _ => {}
    }
}
