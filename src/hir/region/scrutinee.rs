// audited: 2026-11-17
//! Binds the value of each match and destructure that reads a 1-slot container to a fresh name, whose binder counts the read.
//!
//! docs/impl/region/reads.md

use super::reads::reads_a_container;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::value::SymbolId;

/// Give the value of each `match` and each `destructure` that reads a 1-slot
/// container on some path a binding of its own. A pattern binds its names with
/// no binder, so the binding's read is the one the region walk counts
/// (docs/impl/region/reads.md). Runs on the functionalized tree, whose arena
/// says which bindings re-store, lambda bodies included.
pub fn bind_scrutinees(hir: &mut Hir, arena: &mut BindingArena) {
    hir.for_each_child_mut(|child| bind_scrutinees(child, arena));
    let (span, signal) = (hir.span, hir.signal);
    match &mut hir.kind {
        // `(match c …)` becomes `(let [v c] (match v …))`.
        HirKind::Match { value, .. } if reads_a_container(value, arena) => {
            let (v, init) = rebind(value, arena);
            let matched = std::mem::replace(hir, Hir::silent(HirKind::Nil, span));
            *hir = Hir::new(
                HirKind::Let {
                    bindings: vec![(v, init)],
                    body: Box::new(matched),
                },
                span,
                signal,
            );
        }
        // A `define` rather than a `let`, so the pattern's names stay bound in
        // the scope the forms after the `destructure` read them from.
        HirKind::Destructure { value, .. } if reads_a_container(value, arena) => {
            let (v, init) = rebind(value, arena);
            let (init_span, init_signal) = (init.span, init.signal);
            let define = Hir::new(
                HirKind::Define {
                    binding: v,
                    value: Box::new(init),
                },
                init_span,
                init_signal,
            );
            let destructure = std::mem::replace(hir, Hir::silent(HirKind::Nil, span));
            *hir = Hir::new(HirKind::Begin(vec![define, destructure]), span, signal);
        }
        _ => {}
    }
}

/// Replace `value` with a read of a fresh binding, and answer the binding and
/// the value it is bound to.
///
/// The binding is a named temp, not a gensym: a gensym's name is the one a
/// fused loop gives its synthetic mutable, and a read of a binding that shares
/// the reader's name is a version of it, which the walk does not count.
fn rebind(value: &mut Hir, arena: &mut BindingArena) -> (Binding, Hir) {
    let v = arena.temp(SymbolId::of("__scrutinee"));
    arena.get_mut(v).is_immutable = true;
    let read = Hir::silent(HirKind::Var(v), value.span);
    (v, std::mem::replace(value, read))
}
