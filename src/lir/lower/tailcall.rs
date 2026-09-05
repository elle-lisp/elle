// audited: 2026-09-05
//! Whether a HIR body ends in a tail call, read through the control flow and
//! the ANF wrap that hide one.
//!
//! docs/impl/region/relocate.md

use super::*;

impl Lowerer<'_> {
    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). Used to relax the suspension check: a
    /// tail call replaces the frame, so its signal doesn't affect the
    /// enclosing scope's lifetime.
    ///
    /// After the ANF lift, a tail call previously of the form `(f x)`
    /// becomes `(let [t (f x)] t)`. Recognise this single-binding shape
    /// where the body is `Var(b)` and check the init for tail-callness
    /// — `mark_tail_calls` runs before ANF, so `is_tail` is preserved
    /// on the wrapped Call.
    pub(super) fn body_is_tail_call(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { is_tail: true, .. } => true,
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => Self::body_is_tail_call(then_branch) && Self::body_is_tail_call(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| Self::body_is_tail_call(body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|b| Self::body_is_tail_call(b))
            }
            HirKind::Begin(exprs) => exprs.last().is_some_and(Self::body_is_tail_call),
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                // ANF wrap shape: `(let [b e] (var b))` is tail-equivalent
                // to `e`.
                if bindings.len() == 1 {
                    let (b, init) = (&bindings[0].0, &bindings[0].1);
                    if matches!(&body.kind, HirKind::Var(v) if v == b)
                        && Self::body_is_tail_call(init)
                    {
                        return true;
                    }
                }
                Self::body_is_tail_call(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::body_is_tail_call(body)),
            _ => false,
        }
    }
}
