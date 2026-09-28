// audited: 2026-09-28
//! Recognizing the calls a chain is made of, the functions they may splice, and
//! the array base the chain walks.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/inline.md

use super::*;

/// A recognized `(map <f> …)` / `(map-indexed <f> …)` / `(filter <f> …)` /
/// `(take-while <f> …)` / `(drop-while <f> …)` / `(mapcat <f> …)` call: the HOF
/// kind, the function argument, and the collection argument (both borrowed).
/// `None` when `hir` is not a call to one of those canonical stdlib ops with
/// exactly two non-spliced arguments (a user redefinition shadows the name with a
/// non-primitive binding and is excluded).
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

/// A recognized `(fold <lambda> <init> <coll>)` / `(reduce …)` call: the 2-param
/// combinator lambda, the seed `init`, and the collection (all borrowed). `None`
/// when `hir` is not a call to the canonical stdlib `fold`/`reduce` with exactly
/// three non-spliced arguments. `reduce` is `(def reduce fold)` — the same
/// left-fold, recognized by either name. A user redefinition shadows the name with
/// a non-primitive binding and is excluded (the `is_primitive` gate, as for
/// `map`/`filter`). A `fold` is a terminal, like a `count` or a search: its scalar
/// result is not a collection, so nothing chains over it.
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

/// A recognized `(count <pred> <coll>)` call: the 1-parameter predicate and the
/// collection (both borrowed). `None` when `hir` is not a call to the canonical
/// stdlib `count` with exactly two non-spliced arguments; a user redefinition
/// shadows the name with a non-primitive binding and is excluded, as for
/// `map`/`filter`.
///
/// `count` shares `filter`'s two-argument shape but produces a NUMBER, so it is a
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

/// A recognized `(any? <pred> <coll>)` / `(all? …)` / `(find …)` /
/// `(find-index …)` call: which search it is, the 1-parameter predicate, and the
/// collection. `None` when `hir` is not a call to one of the four canonical stdlib
/// searches with exactly two non-spliced arguments; a user redefinition shadows the
/// name with a non-primitive binding and is excluded, as for `map`/`filter`.
///
/// A search shares `filter`'s two-argument shape but produces a SCALAR, so it is a
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

/// The parameters and body of a lambda that qualifies for inlining, or `None`. A
/// qualifying lambda has exactly `arity` fixed parameters (no rest) — one for an
/// element function, two for a `fold` combinator or a `map-indexed`'s
/// position-and-element function — unmutated parameters, and **no nested lambda**
/// in its body (so retyping a parameter to a plain local cannot disturb a capture
/// of it). A raw `%`-intrinsic in the body is
/// admitted only under the lambda's own `(numeric!)` declaration
/// (`body_disqualifies`). These bounds keep the splice a straight
/// `(let [param elem] body)` per parameter with no substitution or cell reasoning.
///
/// A **capture** is admitted: the rewrite moves the literal out of the call and
/// splices its body where that call stood, so every free variable it reads is bound
/// by an enclosing scope of the splice and resolves from the enclosing function with
/// no rename. The exception is a **self-reference** (`CaptureKind::Recursive`),
/// which names the executing closure rather than a binding the frame holds — and
/// fusion removes the closure it would name. What a capture does cost is the composition gate, which `validate_chain`
/// asks separately (`captures_locals`).
pub(in crate::hir::typeinfer::fuse) fn qualifies_lambda<'a>(
    lam: &'a Hir,
    arena: &BindingArena,
    arity: usize,
) -> Option<(&'a [Binding], &'a Hir)> {
    let HirKind::Lambda {
        params,
        rest_param,
        captures,
        body,
        assert_numeric,
        ..
    } = &lam.kind
    else {
        return None;
    };
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

/// Does this function argument read a binding through a **capture** — a name the
/// closure would have carried in its environment, rather than one of its own
/// parameters or a constant the lowerer folds in? Only a call-site lambda literal
/// can answer yes: a fragment refuses a capture outright, its body naming the
/// scope its function was DEFINED in.
///
/// A capture is a cross-element channel the reorder gate cannot see — a mutable
/// local two bodies share, or a mutable value an immutable local names, neither of
/// which raises a signal — so a chain that interleaves two lambdas' calls declines
/// on it.
pub(in crate::hir::typeinfer::fuse) fn captures_locals(lam: &Hir) -> bool {
    matches!(&lam.kind, HirKind::Lambda { captures, .. } if !captures.is_empty())
}

/// Does a lambda body disqualify it from inlining? Two structural hazards, both
/// detected in one walk:
///
/// - **A nested lambda** — retyping the parameter to a plain local (the splice)
///   could disturb a capture of it, and a per-element closure is not the kernel
///   this fusion targets.
/// - **A call-position `%`-intrinsic without a `(numeric!)` declaration**
///   (`declared_numeric == false`) — a raw intrinsic carries an operand proof
///   obligation (docs/intrinsics.md) that a parameter discharges only through
///   the declaration, which floors every parameter at Number. Under a declaration
///   the floor is carried onto the spliced parameter binding, so the site proves
///   in the loop exactly as it did in the function; with no declaration there is
///   nothing to carry and the body stays a plain call. Ordinary numeric kernels written with the
///   stdlib wrappers (`+`/`*`) are plain calls here and never reach this gate.
pub(in crate::hir::typeinfer::fuse) fn body_disqualifies(
    hir: &Hir,
    declared_numeric: bool,
) -> bool {
    if matches!(hir.kind, HirKind::Lambda { .. })
        || (!declared_numeric && matches!(hir.kind, HirKind::Intrinsic { .. }))
    {
        return true;
    }
    let mut found = false;
    hir.for_each_child(|c| found |= body_disqualifies(c, declared_numeric));
    found
}

/// The proven array-ness of a fused chain's base — the fact that selects the
/// terminal's result arm (frozen vs unfrozen), mirroring the stdlib op's own
/// `(if (mutable? coll) acc (freeze acc))`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer::fuse) enum BaseKind {
    /// A proven immutable `array` — the result is frozen. Admits every terminal
    /// and composition (the base cannot be mutated, so an interleaved live walk
    /// preserves the value).
    Immutable,
    /// A proven mutable `@array` — the result is the accumulator unfrozen. Fuses
    /// only a single `map`, `filter` or `mapcat` (see `validate_chain`).
    Mutable,
}

/// Classify `expr` as a statically-proven array base, or `None`. Two proven
/// forms, each carrying its mutability:
///
/// - **A call-site array producer** — a call to a primitive whose declared
///   `RetType` is `Array` (an `[ … ]` literal → the `array` primitive, `->array`,
///   …) → `Immutable`, or `MutableArray` (a `@[ … ]` literal → `@array`, `thaw`,
///   …) → `Mutable`.
/// - **A `Var` alias of one** — a binding whose initializer resolves through the
///   shared init proof (`bases`, built by `prune::concrete_init_keywords`) to the
///   `array` (→ `Immutable`) or `@array` (→ `Mutable`) keyword, following
///   immutable/unmutated/single-init alias chains to a fixpoint.
///
/// The alias proof is the SAME one dead-arm pruning trusts to delete a match arm,
/// so reading it here carries the identical soundness guarantee: the keyword is
/// the value's concrete container type, so `@array` is genuinely mutable at the
/// call site (`freeze` copies to a new immutable value, never mutating its input
/// in place, so a proven-`@array` binding is mutable at every use).
pub(in crate::hir::typeinfer::fuse) fn classify_base(
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
