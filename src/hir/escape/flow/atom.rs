//! The flow atoms and the interprocedural context the source-collection walks
//! carry: the two escape-authority-bearing values (`Atom`) and the arena +
//! arg-return summary (`TailCtx`) an interprocedural tail walk needs.

use rustc_hash::FxHashMap;

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::HirId;

/// An atom of value flow: the thing a (region-transparent) expression *is*.
/// Only these two carry escape-authority — a binding reference or a lambda
/// node. Everything else an expression can evaluate to is either an immediate
/// (no escape to track) or a freshly-minted region the solver names by an
/// allocation site (a `Call` result, an aggregate), which is not a binding or
/// lambda and so propagates no escape backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::hir::escape) enum Atom {
    Binding(Binding),
    Lambda(HirId),
}

/// The context `tail_sources` needs to be interprocedural: the arena (to resolve
/// a callee `Var` and check it is an immutable, unmutated binding) and the
/// arg-return summary (which fixed-param indices each inlinable callee returns).
pub(in crate::hir::escape) struct TailCtx<'a> {
    pub(in crate::hir::escape) arena: &'a BindingArena,
    pub(in crate::hir::escape) arg_return: &'a FxHashMap<Binding, Vec<usize>>,
}
