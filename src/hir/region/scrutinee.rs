// audited: 2026-11-23
//! Binds each match value, destructure value and callee that reads a 1-slot container to a fresh name, whose binder counts the read.
//!
//! docs/impl/region/reads.md

use super::reads::reads_a_container;
use crate::hir::arena::{BindingArena, CALLEE, SCRUTINEE};
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};

/// Give the value of each `match` and each `destructure`, and the callee of
/// each call, that reads a 1-slot container on some path a binding of its own.
/// A pattern binds its names with no binder, and a call holds its callee with
/// none, so the binding's read is the one the region walk counts
/// (docs/impl/region/reads.md). Runs on the functionalized tree, whose arena
/// says which bindings re-store, lambda bodies included.
pub fn bind_scrutinees(hir: &mut Hir, arena: &mut BindingArena) {
    hir.for_each_child_mut(|child| bind_scrutinees(child, arena));
    match &mut hir.kind {
        // `(match c …)` becomes `(let [v c] (match v …))`.
        HirKind::Match { value, .. } if reads_a_container(value, arena) => {
            let (v, init) = rebind(value, arena, SCRUTINEE);
            bind_around(hir, v, init);
        }
        // `(f a)` becomes `(let [g f] (g a))`.
        HirKind::Call { func, .. } if reads_a_container(func, arena) => {
            let (g, init) = rebind(func, arena, CALLEE);
            bind_around(hir, g, init);
        }
        // A `define` rather than a `let`, so the pattern's names stay bound in
        // the scope the forms after the `destructure` read them from.
        HirKind::Destructure { value, .. } if reads_a_container(value, arena) => {
            let (span, signal) = (hir.span, hir.signal);
            let (v, init) = rebind(value, arena, SCRUTINEE);
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

/// Wrap `hir` in a `let` that binds `v` to `init`, with `hir`'s span and
/// signal.
fn bind_around(hir: &mut Hir, v: Binding, init: Hir) {
    let (span, signal) = (hir.span, hir.signal);
    let body = std::mem::replace(hir, Hir::silent(HirKind::Nil, span));
    *hir = Hir::new(
        HirKind::Let {
            bindings: vec![(v, init)],
            body: Box::new(body),
        },
        span,
        signal,
    );
}

/// Replace `value` with a read of a fresh binding named `name`, and answer the
/// binding and the value it is bound to.
///
/// The binding is a named temp, not a gensym: a gensym's name is the one a
/// fused loop gives its synthetic mutable, and a read of a binding that shares
/// the reader's name is a version of it, which the walk does not count.
fn rebind(value: &mut Hir, arena: &mut BindingArena, name: &'static str) -> (Binding, Hir) {
    let v = arena.temp(name);
    arena.get_mut(v).is_immutable = true;
    let read = Hir::silent(HirKind::Var(v), value.span);
    (v, std::mem::replace(value, read))
}
