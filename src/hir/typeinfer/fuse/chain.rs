use super::*;

/// The higher-order collection op a fused chain is built from, and the kind of
/// each *stage* in the unified pipeline (`Build::element`). Both take
/// `(lambda, coll)` and share the `(get`/`push`/`freeze)` index-walk over `coll`'s
/// array arm; they differ only in how a stage handles the threaded element value:
/// a `Map` stage transforms it and threads the result on, a `Filter` stage guards
/// the rest of the pipeline behind its predicate (an `if`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hof {
    Map,
    Filter,
}

impl Hof {
    /// The canonical stdlib name this op is recognized by.
    pub(super) fn from_name(name: &str) -> Option<Hof> {
        match name {
            "map" => Some(Hof::Map),
            "filter" => Some(Hof::Filter),
            _ => None,
        }
    }
}

/// How a fused chain collects its per-element results — the pipeline's
/// **terminal**, realized by the innermost base case of `Build::element`. The
/// `map`/`filter` pipeline stages are identical for both terminals; only the
/// accumulator setup (`build_loop`) and the base case differ.
///
/// - **Collect** — a `map`/`filter`-only chain: fill a fresh `@array` by `push`.
///   `unfrozen` picks the result arm (the mutable-array arm): an immutable base
///   `freeze`s the accumulator to an immutable result; a mutable `@array` base
///   returns it unfrozen (type-preserving, mirroring the stdlib op's own
///   `(if (mutable? coll) acc (freeze acc))`).
/// - **Fold** — a `fold`/`reduce` at the head: a **scalar** accumulator seeded by
///   `init`, updated `(assign acc (f acc elem))` per surviving element, whose final
///   value is the result (no `@array`, no `freeze`). `f` is the 2-parameter
///   combinator lambda, moved in whole (`acc_param`, `elem_param`, `body`). The
///   payload is boxed so the empty `Collect` does not inflate every `Terminal`.
pub(super) enum Terminal {
    Collect { unfrozen: bool },
    Fold(Box<FoldTerminal>),
}

/// The moved-out parts of a `fold`/`reduce` terminal (the boxed `Terminal::Fold`
/// payload): the seed `init` and the combinator's two params + body.
pub(super) struct FoldTerminal {
    pub(super) init: Hir,
    pub(super) acc_param: Binding,
    pub(super) elem_param: Binding,
    pub(super) body: Hir,
}

/// A recognized `(map <lambda> …)` / `(filter <lambda> …)` call: the HOF kind,
/// the lambda argument, and the collection argument (both borrowed). `None` when
/// `hir` is not a call to the canonical stdlib `map`/`filter` with exactly two
/// non-spliced arguments (a user redefinition shadows the name with a
/// non-primitive binding and is excluded).
pub(super) fn fusable_hof_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
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
    let hof = Hof::from_name(symbol_names.get(&bi.name.0)?)?;
    Some((hof, &args[0].expr, &args[1].expr))
}

/// A recognized `(fold <lambda> <init> <coll>)` / `(reduce …)` call: the 2-param
/// combinator lambda, the seed `init`, and the collection (all borrowed). `None`
/// when `hir` is not a call to the canonical stdlib `fold`/`reduce` with exactly
/// three non-spliced arguments. `reduce` is `(def reduce fold)` — the same
/// left-fold, recognized by either name. A user redefinition shadows the name with
/// a non-primitive binding and is excluded (the `is_primitive` gate, as for
/// `map`/`filter`). `fold` is the only op that may be the chain's outermost
/// terminal — its scalar result is not a collection, so nothing chains over it.
pub(super) fn fusable_fold_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
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
    let name = symbol_names.get(&bi.name.0)?;
    if name != "fold" && name != "reduce" {
        return None;
    }
    Some((&args[0].expr, &args[1].expr, &args[2].expr))
}

/// The parameters and body of a lambda that qualifies for inlining, or `None`. A
/// qualifying lambda is a literal with exactly `arity` fixed parameters (no rest)
/// — one for a `map`/`filter` predicate, two for a `fold` combinator — no captures
/// (its body references only the parameters and globals, so splicing at the call
/// site is always in scope), unmutated parameters, and **no nested lambda** in its
/// body (so retyping a parameter to a plain local cannot disturb a capture of it).
/// A raw `%`-intrinsic in the body is admitted only under the lambda's own
/// `(numeric!)` declaration (`body_disqualifies`). These bounds keep the splice a
/// straight `(let [param elem] body)` per parameter with no substitution or cell
/// reasoning.
pub(super) fn qualifies_lambda<'a>(
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
    if rest_param.is_some() || params.len() != arity || !captures.is_empty() {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || body_disqualifies(body, *assert_numeric) {
        return None;
    }
    Some((params, body))
}

/// Does a lambda body disqualify it from inlining? Two structural hazards, both
/// detected in one walk:
///
/// - **A nested lambda** — retyping the parameter to a plain local (the splice)
///   could disturb a capture of it, and a per-element closure is not the kernel
///   this fusion targets.
/// - **A call-position `%`-intrinsic without a `(numeric!)` declaration**
///   (`declared_numeric == false`) — a raw intrinsic carries an operand proof
///   obligation (`docs/intrinsics.md`) that a parameter discharges only through
///   the declaration, which floors every parameter at Number. Under a declaration
///   the floor is carried onto the spliced parameter binding, so the site proves
///   in the loop exactly as it did in the function (docs/impl/dissolution.md
///   § "Raw `%`-intrinsic bodies"); with no declaration there is nothing to carry
///   and the body stays a plain call. Ordinary numeric kernels written with the
///   stdlib wrappers (`+`/`*`) are plain calls here and never reach this gate.
pub(super) fn body_disqualifies(hir: &Hir, declared_numeric: bool) -> bool {
    if matches!(hir.kind, HirKind::Lambda { .. })
        || (!declared_numeric && matches!(hir.kind, HirKind::Intrinsic { .. }))
    {
        return true;
    }
    let mut found = false;
    hir.for_each_child(|c| found |= body_disqualifies(c, declared_numeric));
    found
}

/// A validated fusable chain, ready for `take_chain`: whether the outermost op is
/// a `fold`/`reduce` terminal (a scalar accumulator), the inner `map`/`filter`
/// pipeline kinds in the order the walk encounters them (OUTER→INNER), and whether
/// the base is a mutable `@array` (so a Collect terminal emits the accumulator
/// unfrozen — the mutable-array arm).
pub(super) struct ChainPlan {
    pub(super) fold: bool,
    pub(super) kinds: Vec<Hof>,
    pub(super) mutable_base: bool,
}

/// Validate that `hir` is a fusable HOF chain and return its plan. The chain is an
/// optional outermost `fold`/`reduce` (the scalar terminal) over a `map`/`filter`
/// pipeline (in any mix) bottoming out at a proven immutable array. Every lambda
/// qualifies (`qualifies_lambda`, arity 1 for `map`/`filter`, 2 for `fold`); and
/// for a **composition** — total op count ≥ 2, where the fold counts as an op —
/// every lambda body is `reorder_safe` (the reordering gate; see the module doc).
/// A lone `fold` (or a lone `map`/`filter`) is a single op and carries no reorder
/// requirement: a fold threads its accumulator strictly in element order, exactly
/// the stdlib fold. A non-reorder-safe stage declines the whole composition, and
/// the pre-order recursion (`rewrite`) still fuses its inner reorder-safe run.
///
/// The walk stops at the first node that is not a fusable HOF call; that node is
/// the base candidate. If it is not a proven immutable array (e.g. the chain never
/// reaches one, or a lambda failed `qualifies_lambda`), fusion declines and the
/// recursion retries at the inner calls.
pub(super) fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    bases: &FxHashMap<Binding, &'static str>,
    fns: &FnResolver,
) -> Option<ChainPlan> {
    let mut all_silent = true;
    let mut ops = 0usize;
    let mut cur = hir;

    // The optional outermost fold/reduce terminal (2-param combinator).
    let fold = if let Some((lam, _init, coll)) = fusable_fold_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fns.body_signal(lam, arena, 2)?);
        ops += 1;
        cur = coll;
        true
    } else {
        false
    };

    // The inner map/filter pipeline (1-param functions).
    let mut kinds = Vec::new();
    while let Some((hof, lam, coll)) = fusable_hof_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fns.body_signal(lam, arena, 1)?);
        ops += 1;
        kinds.push(hof);
        cur = coll;
    }

    if ops == 0 {
        return None;
    }
    let base = classify_base(cur, arena, symbol_names, bases)?;
    // A mutable `@array` base fuses only a single `map`/`filter`: the fused loop
    // walks the base LIVE, which matches the stdlib op exactly for one op, but a
    // `fold` (which snapshots via `->array`) or a composition (whose staged ops
    // each run to completion over a fresh array) would diverge from an interleaved
    // live walk under a mutating lambda (dissolution.md § "The mutable-array arm").
    // The pre-order recursion still fuses the innermost single op of a declined
    // mutable composition.
    let mutable_base = base == BaseKind::Mutable;
    if mutable_base && (fold || kinds.len() != 1) {
        return None;
    }
    if ops >= 2 && !all_silent {
        return None;
    }
    Some(ChainPlan {
        fold,
        kinds,
        mutable_base,
    })
}

/// May a lambda body be safely reordered against sibling per-element work in a
/// composition? A composition interleaves the transforms rather than running
/// each to completion, so a body is reorder-safe only if it has no genuine
/// **sequencing** effect: no yield, I/O, emit, FFI, OS-signal, or halt, and it
/// propagates no parameter signal. `SIG_ERROR` is deliberately permitted —
/// error reordering changes only *which* of several errors surfaces first (each
/// still surfaces as an error), and a dissolvable numeric kernel over proven
/// data does not error at all; refusing it would forbid every arithmetic
/// composition, which is exactly the tower shape this fusion exists to collapse.
pub(super) fn reorder_safe(sig: Signal) -> bool {
    sig.bits.subtract(SIG_ERROR).is_empty() && sig.propagates == 0
}

/// The proven array-ness of a fused chain's base — the fact that selects the
/// terminal's result arm (frozen vs unfrozen), mirroring the stdlib op's own
/// `(if (mutable? coll) acc (freeze acc))`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseKind {
    /// A proven immutable `array` — the result is frozen. Admits `fold` and
    /// compositions (the base cannot be mutated, so an interleaved live walk
    /// preserves the value).
    Immutable,
    /// A proven mutable `@array` — the result is the accumulator unfrozen. Fuses
    /// only a single `map`/`filter` (see `validate_chain` and dissolution.md
    /// § "The mutable-array arm").
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
pub(super) fn classify_base(
    expr: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
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
    let name = symbol_names.get(&bi.name.0)?;
    match crate::primitives::registration::def_by_name(name).map(|d| d.ret) {
        Some(RetType::Array) => Some(BaseKind::Immutable),
        Some(RetType::MutableArray) => Some(BaseKind::Mutable),
        _ => None,
    }
}

/// Consume a validated chain, returning its **terminal** (Collect or the fold
/// combinator), its per-element `map`/`filter` **stages** (`(hof, param, body)`) in
/// **application order** (innermost op first), and the base collection expression.
/// `plan.fold` and `plan.kinds` are the chain's shape in outer→inner order (from
/// `validate_chain`); the fold is peeled first (it wraps the pipeline), then the
/// map/filter ops. Each op's function is resolved by `take_fn_parts` — moved (a
/// lambda literal) or cloned fresh (a named template). Validation guarantees the
/// structure, so every destructuring is total.
pub(super) fn take_chain(
    mut expr: Hir,
    plan: ChainPlan,
    arena: &mut BindingArena,
    fns: &FnResolver,
) -> (Terminal, Vec<(Hof, Binding, Hir)>, Hir) {
    let terminal = if plan.fold {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a fold call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("fold has 3 args").expr;
        let init = it.next().expect("fold has 3 args").expr;
        let coll = it.next().expect("fold has 3 args").expr;
        let (params, body) = fns.take_parts(lam, arena);
        expr = coll;
        Terminal::Fold(Box::new(FoldTerminal {
            init,
            acc_param: params[0],
            elem_param: params[1],
            body,
        }))
    } else {
        // The mutable-array arm: a mutable `@array` base returns the accumulator
        // unfrozen (validate_chain proves a mutable base is a lone map/filter, so
        // it is always a Collect, never paired with a Fold).
        Terminal::Collect {
            unfrozen: plan.mutable_base,
        }
    };

    let mut stages = Vec::with_capacity(plan.kinds.len());
    for hof in plan.kinds {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a HOF call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("HOF has 2 args").expr;
        let coll = it.next().expect("HOF has 2 args").expr;
        let (params, body) = fns.take_parts(lam, arena);
        stages.push((hof, params[0], body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    stages.reverse();
    (terminal, stages, expr)
}
