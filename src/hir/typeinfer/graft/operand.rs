// audited: 2026-10-20
//! Which operand is written in place of its parameter, and which a `let` binds first.
//!
//! docs/impl/graft/operands.md

use super::Grafter;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};

impl Grafter<'_> {
    /// Is this operand written in place of its parameter? It is when it holds
    /// one value from the call to the body's last read of it: an immediate
    /// literal, a binding nothing writes, or a written binding that no closure
    /// captures and none of the operands `later` writes. A closed body holds
    /// no `assign` and no lambda, so nothing else can write a binding between
    /// the call and a read. Anything else is bound by a `let`, so it is
    /// evaluated once and where the call evaluated it.
    pub(super) fn is_direct(&self, operand: &Hir, later: &[Hir]) -> bool {
        match &operand.kind {
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_) => true,
            // The test is on writes, not on immutability: a `match` pattern's
            // variable is never marked immutable.
            HirKind::Var(b) => {
                let inner = self.arena.get(*b);
                !inner.is_mutated
                    || (!inner.closure_may_write() && !later.iter().any(|h| writes(h, *b)))
            }
            _ => false,
        }
    }
}

/// Does `h` write `b` anywhere inside it? An `assign` to it does, and so does
/// a store into its cell. A lambda inside `h` that writes it has captured it,
/// which `closure_may_write` already answers for.
fn writes(h: &Hir, b: Binding) -> bool {
    let mut found = match &h.kind {
        HirKind::Assign { target, .. } => *target == b,
        HirKind::SetCell { cell, .. } => matches!(cell.kind, HirKind::Var(c) if c == b),
        _ => false,
    };
    h.for_each_child(|c| found |= writes(c, b));
    found
}
