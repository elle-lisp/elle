// audited: 2026-11-17
//! What reads out of a 1-slot container: the whole value or an element of it, on some path of a form.
//!
//! docs/impl/region/reads.md

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind, IntrinsicOp};
use crate::value::SymbolId;

/// A read out of a 1-slot container, found at the end of one path of a form.
pub(crate) enum ContainerRead<'h> {
    /// The whole value: a `Var` of the container, or the `DerefCell` around
    /// one.
    Whole(Binding),
    /// An element: a `%get`, `%first` or `%rest` whose operand is itself a
    /// container read.
    Element { op: IntrinsicOp, operand: &'h Hir },
}

/// Classify `h` itself as a container read. `version` names a whole read that
/// is no read: a binding of that name is a version of the reader
/// (docs/impl/region/reads.md). An element read is never a version.
pub(crate) fn container_read<'h>(
    h: &'h Hir,
    arena: &BindingArena,
    version: Option<SymbolId>,
) -> Option<ContainerRead<'h>> {
    match &h.kind {
        HirKind::Var(b) => {
            let info = arena.get(*b);
            (info.is_one_slot_container() && Some(info.name) != version)
                .then_some(ContainerRead::Whole(*b))
        }
        HirKind::DerefCell { cell } => container_read(cell, arena, version),
        HirKind::Intrinsic { op, args }
            if matches!(
                op,
                IntrinsicOp::Get | IntrinsicOp::First | IntrinsicOp::Rest
            ) =>
        {
            let operand = args.first()?;
            container_read(operand, arena, None)?;
            Some(ContainerRead::Element { op: *op, operand })
        }
        _ => None,
    }
}

/// Does `h` read out of a 1-slot container on some path?
pub(crate) fn reads_a_container(h: &Hir, arena: &BindingArena) -> bool {
    if container_read(h, arena, None).is_some() {
        return true;
    }
    let mut reads = false;
    for_each_path(h, |p| reads |= reads_a_container(p, arena));
    reads
}

/// Visit each sub-expression whose value `h` hands up as its own, one per path:
/// a statement wrapper's tail, and every arm of a branch. Any other form
/// produces its value itself and visits nothing.
pub(crate) fn for_each_path<'h>(h: &'h Hir, mut visit: impl FnMut(&'h Hir)) {
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
