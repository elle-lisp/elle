//! Tail-call body recognition: when a scope body is a tail call (or control
//! flow whose every result position is one), RegionExit fires BEFORE the tail
//! call runs, so its result never flows through the scope.

use super::*;

impl RegionInference {
    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). When the body is a tail call, RegionExit
    /// fires BEFORE the tail call executes, so the tail call's result does
    /// not flow through the scope — skip the body escape constraint.
    fn _is_tail_call_body(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { is_tail: true, .. } => true,
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => Self::_is_tail_call_body(then_branch) && Self::_is_tail_call_body(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| Self::_is_tail_call_body(body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|b| Self::_is_tail_call_body(b))
            }
            HirKind::Begin(exprs) => exprs.last().is_some_and(Self::_is_tail_call_body),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::_is_tail_call_body(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::_is_tail_call_body(body)),
            _ => false,
        }
    }
}
