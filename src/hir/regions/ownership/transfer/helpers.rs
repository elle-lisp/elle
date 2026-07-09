//! Structural/ANF unwrapping helpers shared by the use index and the driver:
//! how a callee's declared effect and symbol are read, and how the ANF/cell
//! wrappers are descended to the value a position actually consumes.

use super::*;

/// The declared effect of a call's callee, under the same immutable-unshadowed
/// condition the region walk applies (`RegionInference::call_effect`).
pub(super) fn callee_effect(
    func: &Hir,
    arena: &BindingArena,
    cc: &CallClassification,
) -> Option<crate::primitives::def::RegionEffect> {
    callee_symbol(func, arena).and_then(|sym| cc.effects.get(&sym).copied())
}

/// The callee's SymbolId, when it is an immutable, never-mutated binding.
pub(super) fn callee_symbol(func: &Hir, arena: &BindingArena) -> Option<crate::value::SymbolId> {
    if let HirKind::Var(b) = &unwrap_cell(func).kind {
        let bi = arena.get(*b);
        if bi.is_immutable && !bi.is_mutated {
            return Some(bi.name);
        }
    }
    None
}

/// Descend the structural/ANF wrappers to the expression a position actually
/// consumes: the ANF lift names an allocating argument in place —
/// `(fiber/new (fn …) mask)` becomes `(fiber/new (let [t (fn …)] t) mask)` — so
/// the value at an arg position sits at the wrapper's tail.
pub(super) fn anf_tail(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => anf_tail(body),
        HirKind::Begin(es) => es.last().map_or(h, anf_tail),
        _ => h,
    }
}

/// Unwrap the cell wrappers a captured binding's flow wears: `MakeCell` around
/// its init, `DerefCell` around each read (both lowerer-transparent — the value
/// identity is unchanged). A captured producer's Define init and its call-site
/// reads must resolve through them.
pub(super) fn unwrap_cell(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::MakeCell { value } => unwrap_cell(value),
        HirKind::DerefCell { cell } => unwrap_cell(cell),
        _ => h,
    }
}
