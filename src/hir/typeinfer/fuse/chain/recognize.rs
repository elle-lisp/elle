// audited: 2026-10-21
//! Recognizing the calls a chain is made of, the functions they may splice, and
//! the array base the chain walks.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/inline.md

use super::*;
use crate::hir::Lambda;

/// A recognized pipeline-op call — `map`, `map-indexed`, `filter`, `take-while`,
/// `drop-while` or `mapcat` — as the op, its function argument and its collection
/// argument. `None` when `hir` is not a call to one of those canonical stdlib ops
/// with exactly two non-spliced arguments. A user redefinition shadows the name
/// with a binding that is not `is_primitive`, and is excluded.
pub(in crate::hir::typeinfer::fuse) fn fusable_hof_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
) -> Option<(Hof, &'a Hir, &'a Hir)> {
    let HirKind::Call { func, args, .. } = &hir.kind else {
        return None;
    };
    if args.len() != 2 || args.iter().any(|a| a.spliced) {
        return None;
    }
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive {
        return None;
    }
    let hof = Hof::from_symbol(bi.name)?;
    Some((hof, &args[0].expr, &args[1].expr))
}

/// A recognized `(fold f init coll)` or `(reduce …)` call: the function
/// argument, the seed and the collection. `None` when `hir` is not a call to
/// the canonical stdlib `fold` or `reduce` with exactly three non-spliced
/// arguments. `reduce` is `(def reduce fold)`, so either name reads the same
/// left fold. A user redefinition shadows the name with a binding that is not
/// `is_primitive`, and is excluded.
pub(in crate::hir::typeinfer::fuse) fn fusable_fold_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
) -> Option<(&'a Hir, &'a Hir, &'a Hir)> {
    let HirKind::Call { func, args, .. } = &hir.kind else {
        return None;
    };
    if args.len() != 3 || args.iter().any(|a| a.spliced) {
        return None;
    }
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive {
        return None;
    }
    if bi.name != SymbolId::of("fold") && bi.name != SymbolId::of("reduce") {
        return None;
    }
    Some((&args[0].expr, &args[1].expr, &args[2].expr))
}

/// A recognized `(count pred coll)` call: the predicate and the collection.
/// `None` when `hir` is not a call to the canonical stdlib `count` with exactly
/// two non-spliced arguments, or when a user redefinition shadows the name.
///
/// `count` takes `filter`'s two arguments but produces a number, so it is a
/// terminal rather than a stage: `Hof::from_symbol` never answers for it, and
/// `validate_chain` asks this before the pipeline walk starts.
pub(in crate::hir::typeinfer::fuse) fn fusable_count_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
) -> Option<(&'a Hir, &'a Hir)> {
    let HirKind::Call { func, args, .. } = &hir.kind else {
        return None;
    };
    if args.len() != 2 || args.iter().any(|a| a.spliced) {
        return None;
    }
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive || bi.name != SymbolId::of("count") {
        return None;
    }
    Some((&args[0].expr, &args[1].expr))
}

/// A recognized `(any? pred coll)`, `(all? …)`, `(find …)` or `(find-index …)`
/// call: which search it is, the predicate and the collection. `None` when
/// `hir` is not a call to one of the four canonical stdlib searches with
/// exactly two non-spliced arguments, or when a user redefinition shadows the
/// name.
///
/// A search takes `filter`'s two arguments but produces a scalar, so it is a
/// terminal rather than a stage: `Hof::from_symbol` never answers for one, and
/// `validate_chain` asks this before the pipeline walk starts.
pub(in crate::hir::typeinfer::fuse) fn fusable_search_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
) -> Option<(Search, &'a Hir, &'a Hir)> {
    let HirKind::Call { func, args, .. } = &hir.kind else {
        return None;
    };
    if args.len() != 2 || args.iter().any(|a| a.spliced) {
        return None;
    }
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive {
        return None;
    }
    let search = Search::from_symbol(bi.name)?;
    Some((search, &args[0].expr, &args[1].expr))
}

/// The parameters and body of a lambda literal that qualifies for splicing, or
/// `None`. It takes exactly `arity` fixed parameters and no rest list, mutates
/// none of them, and does not refer to itself (`CaptureKind::Recursive`): the
/// self-reference names the closure that fusion removes. Its body passes
/// `body_disqualifies`. Every other capture is admitted, because the body is
/// spliced where the literal was written, and `captures_locals` answers the
/// composition gate separately.
pub(in crate::hir::typeinfer::fuse) fn qualifies_lambda<'a>(
    lam: &'a Hir,
    arena: &BindingArena,
    arity: usize,
) -> Option<(&'a [Binding], &'a Hir)> {
    let Lambda {
        params,
        rest_param,
        captures,
        body,
        assert_numeric,
        ..
    } = lam.kind.as_lambda()?;
    if rest_param.is_some() || params.len() != arity {
        return None;
    }
    if captures
        .iter()
        .any(|c| matches!(c.kind, CaptureKind::Recursive { .. }))
    {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || body_disqualifies(body, *assert_numeric) {
        return None;
    }
    Some((params, body))
}

/// Does this function argument capture a binding? Only a lambda literal can: a
/// fragment does not close over a body that names an enclosing runtime local.
pub(in crate::hir::typeinfer::fuse) fn captures_locals(lam: &Hir) -> bool {
    matches!(lam.kind.as_lambda(), Some(lambda) if !lambda.captures.is_empty())
}

/// Does a lambda body disqualify it from splicing? It does when it holds either
/// of these:
///
/// - **A nested lambda.** The splice turns a parameter into a plain local,
///   which could disturb a capture of it, and a closure per element is not the
///   kernel this fusion targets.
/// - **A call-position `%`-intrinsic without `(numeric!)`**
///   (`declared_numeric == false`). Only the declaration's floor on the
///   parameters proves the intrinsic's operands, and the floor travels with the
///   parameter binding into the loop. A kernel written with the stdlib wrappers,
///   such as `+`, holds plain calls here, because the graft runs after fusion.
pub(in crate::hir::typeinfer::fuse) fn body_disqualifies(
    hir: &Hir,
    declared_numeric: bool,
) -> bool {
    if matches!(hir.kind, HirKind::Lambda(_))
        || (!declared_numeric && matches!(hir.kind, HirKind::Intrinsic { .. }))
    {
        return true;
    }
    let mut found = false;
    hir.for_each_child(|c| found |= body_disqualifies(c, declared_numeric));
    found
}

/// The proven array-ness of a fused chain's base: the fact that selects the
/// terminal's result arm, frozen or unfrozen, as the stdlib op's own
/// `(if (mutable? coll) acc (freeze acc))` does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer) enum BaseKind {
    /// A proven immutable `array` — the result is frozen. Admits `fold` and
    /// compositions (the base cannot be mutated, so an interleaved live walk
    /// preserves the value).
    Immutable,
    /// A proven mutable `@array` — the result is the accumulator unfrozen. Fuses
    /// only a single `map`, `filter` or `mapcat` (see `validate_chain`).
    Mutable,
}

/// Classify `expr` as a proven array base, or `None`:
///
/// - **A call to an unshadowed primitive** whose declared `RetType` is `Array`
///   (the `array` call an `[ … ]` literal makes, or `->array`) is `Immutable`.
///   One whose `RetType` is `MutableArray` (the `@array` call a `@[ … ]`
///   literal makes) is `Mutable`.
/// - **A `Var`** is what `bases` maps its binding to: `array` is `Immutable`
///   and `@array` is `Mutable`. `prune::concrete_init_keywords` builds that
///   map, following immutable, unmutated, single-init alias chains.
///
/// `freeze` copies into a new value and never changes its input, so a binding
/// proven `@array` is mutable at every use.
pub(in crate::hir::typeinfer) fn classify_base(
    expr: &Hir,
    arena: &BindingArena,
    bases: &FxHashMap<Binding, &'static str>,
) -> Option<BaseKind> {
    if let HirKind::Var(b) = &expr.kind {
        return match bases.get(b) {
            Some(&"array") => Some(BaseKind::Immutable),
            Some(&"@array") => Some(BaseKind::Mutable),
            _ => None,
        };
    }
    let HirKind::Call { func, .. } = &expr.kind else {
        return None;
    };
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive || !bi.is_immutable || bi.is_mutated {
        return None;
    }
    match crate::primitives::registration::def_by_symbol(bi.name).map(|d| d.ret) {
        Some(RetType::Array) => Some(BaseKind::Immutable),
        Some(RetType::MutableArray) => Some(BaseKind::Mutable),
        _ => None,
    }
}
