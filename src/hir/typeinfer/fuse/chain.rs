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

    /// What this op does as a pipeline stage.
    pub(super) fn stage(self) -> StageKind {
        match self {
            Hof::Map => StageKind::Transform,
            Hof::Filter => StageKind::Keep,
        }
    }
}

/// What one stage of the unified pipeline does with the element value threaded
/// into it (`Build::element`). `map` and `filter` supply the first two; the third
/// is the guard an `all?` terminal appends, whose answer is decided by an element
/// the predicate REJECTS (docs/impl/dissolution.md § "Search — the terminal that
/// stops early"). A guard is one `if` either way — the two kinds differ only in
/// which branch carries the rest of the pipeline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StageKind {
    /// A `map`: transform the threaded value and thread the result on.
    Transform,
    /// A `filter`, and the guard a `count`/`any?`/`find`/`find-index` terminal
    /// appends: continue the pipeline for the elements the predicate ADMITS.
    Keep,
    /// The guard an `all?` terminal appends: continue for the elements the
    /// predicate REJECTS.
    Reject,
}

/// The four short-circuiting stdlib searches. Each takes a `(predicate,
/// collection)` shape and answers a scalar about the FIRST element its predicate
/// decides, reading no element past it — so each is a terminal, and the loop the
/// four share leaves early through a sentinel its condition reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Search {
    /// `any?` — `true` at the first admitted element, `false` if none is.
    Any,
    /// `all?` — `false` at the first rejected element, `true` if none is.
    All,
    /// `find` — the first admitted element itself, `nil` if none is.
    Find,
    /// `find-index` — the position of the first admitted element, `nil` if none is.
    FindIndex,
}

impl Search {
    /// The canonical stdlib name this search is recognized by.
    pub(super) fn from_name(name: &str) -> Option<Search> {
        match name {
            "any?" => Some(Search::Any),
            "all?" => Some(Search::All),
            "find" => Some(Search::Find),
            "find-index" => Some(Search::FindIndex),
            _ => None,
        }
    }

    /// Which side of the predicate decides this search's answer, as the stage its
    /// predicate becomes: `all?` is decided by a rejected element, the other three
    /// by an admitted one.
    pub(super) fn guard(self) -> StageKind {
        match self {
            Search::All => StageKind::Reject,
            _ => StageKind::Keep,
        }
    }
}

/// How a fused chain collects its per-element results — the pipeline's
/// **terminal**, realized by the innermost base case of `Build::element`. The
/// `map`/`filter` pipeline stages are identical for every terminal; only the
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
/// - **Count** — a `count` at the head: a **scalar** accumulator seeded at 0 and
///   incremented once per surviving element. The count's own predicate is not
///   carried here — it becomes the pipeline's last guard stage — so the terminal
///   itself is a bare tally (docs/impl/dissolution.md § "Count — the terminal that
///   is a guard plus a tally").
/// - **Search** — an `any?`/`all?`/`find`/`find-index` at the head: a **scalar**
///   accumulator seeded with the answer for "no element decided it", written once
///   by the deciding element, which also clears the sentinel the loop condition
///   reads so the walk stops there. Its predicate becomes the pipeline's last
///   guard stage too, so the terminal carries only which search it is.
pub(super) enum Terminal {
    Collect { unfrozen: bool },
    Fold(Box<FoldTerminal>),
    Count,
    Search(Search),
}

/// Which op sits at the head of a validated chain — the shape `take_chain` peels
/// before the `map`/`filter` pipeline. `Fold`, `Count` and `Search` are the scalar
/// terminals; `Collect` means the chain is `map`/`filter` all the way up and its
/// result is a fresh array.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalOp {
    Collect,
    Fold,
    Count,
    Search(Search),
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

/// A recognized `(count <pred> <coll>)` call: the 1-parameter predicate and the
/// collection (both borrowed). `None` when `hir` is not a call to the canonical
/// stdlib `count` with exactly two non-spliced arguments; a user redefinition
/// shadows the name with a non-primitive binding and is excluded, as for
/// `map`/`filter`.
///
/// `count` shares `filter`'s two-argument shape but produces a NUMBER, so it is a
/// terminal rather than a stage: `Hof::from_name` never answers for it, and
/// `validate_chain` asks this before the pipeline walk starts.
pub(super) fn fusable_count_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Option<(&'a Hir, &'a Hir)> {
    let HirKind::Call { func, args, .. } = &hir.kind else {
        return None;
    };
    if args.len() != 2 || args.iter().any(|a| a.spliced) {
        return None;
    }
    let callee = unwrap_callee_binding(func)?;
    let bi = arena.get(callee);
    if !bi.is_primitive || symbol_names.get(&bi.name.0)? != "count" {
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
/// terminal rather than a stage: `Hof::from_name` never answers for one, and
/// `validate_chain` asks this before the pipeline walk starts.
pub(super) fn fusable_search_parts<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
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
    let search = Search::from_name(symbol_names.get(&bi.name.0)?)?;
    Some((search, &args[0].expr, &args[1].expr))
}

/// The parameters and body of a lambda that qualifies for inlining, or `None`. A
/// qualifying lambda is a literal with exactly `arity` fixed parameters (no rest)
/// — one for a `map`/`filter`/`count`/search element function, two for a `fold`
/// combinator — no captures
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

/// A validated fusable chain, ready for `take_chain`: which op heads it (a scalar
/// terminal, or nothing but the `map`/`filter` pipeline), the inner `map`/`filter`
/// pipeline kinds in the order the walk encounters them (OUTER→INNER), and whether
/// the base is a mutable `@array` (so a Collect terminal emits the accumulator
/// unfrozen — the mutable-array arm).
pub(super) struct ChainPlan {
    pub(super) terminal: TerminalOp,
    pub(super) kinds: Vec<Hof>,
    pub(super) mutable_base: bool,
}

/// Validate that `hir` is a fusable HOF chain and return its plan. The chain is an
/// optional outermost scalar terminal — a `fold`/`reduce`, a `count`, or a search —
/// over a `map`/`filter` pipeline (in any mix) bottoming out at a proven immutable
/// array. Every function qualifies (`qualifies_lambda`, arity 1 for
/// `map`/`filter`/`count`/a search, 2 for `fold`); and for a **composition** — total
/// op count ≥ 2, where the terminal counts as an op — every body is `reorder_safe`
/// (the reordering gate; see the module doc). A lone terminal (or a lone
/// `map`/`filter`) is a single op and carries no reorder requirement: a fold threads
/// its accumulator strictly in element order and a count applies its predicate left
/// to right, exactly as the stdlib ops do. A non-reorder-safe stage declines the
/// whole composition, and the pre-order recursion (`rewrite`) still fuses its inner
/// reorder-safe run. A search never reaches the gate at all: it takes no prefix, for
/// the stronger reason below.
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

    // The optional outermost scalar terminal: a fold/reduce (2-param combinator), a
    // count, or one of the four searches (1-param predicate). Asked before the
    // pipeline walk, so a `count` or a search — each of which wears a `filter`'s
    // two-argument shape — is never read as a stage.
    let terminal = if let Some((lam, _init, coll)) = fusable_fold_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fns.body_signal(lam, arena, 2)?);
        ops += 1;
        cur = coll;
        TerminalOp::Fold
    } else if let Some((pred, coll)) = fusable_count_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fns.body_signal(pred, arena, 1)?);
        ops += 1;
        cur = coll;
        TerminalOp::Count
    } else if let Some((search, pred, coll)) = fusable_search_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fns.body_signal(pred, arena, 1)?);
        ops += 1;
        cur = coll;
        TerminalOp::Search(search)
    } else {
        TerminalOp::Collect
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
    // A search fuses only as a LONE op. A staged `(any? p (map f xs))` runs `f`
    // over the WHOLE input before `any?` examines an element, where one fused loop
    // stops at the first deciding element and so omits `f` on every later one —
    // work the staged form performs, not merely work it performs in another order.
    // The composition gate's argument covers reordering (and, for `SIG_ERROR`,
    // which error surfaces), never work that no longer runs (dissolution.md
    // § "Search — the terminal that stops early"). The pre-order recursion still
    // fuses the declined chain's inner run.
    if matches!(terminal, TerminalOp::Search(_)) && !kinds.is_empty() {
        return None;
    }
    let base = classify_base(cur, arena, symbol_names, bases)?;
    // A mutable `@array` base fuses only a single `map`/`filter`: the fused loop
    // walks the base LIVE against a `len` captured once, which matches the stdlib op
    // exactly for one op. A `fold` (which snapshots via `->array`), a `count` (which
    // re-reads `(length coll)` every iteration), and a composition (whose staged ops
    // each run to completion over a fresh array) would each diverge from an
    // interleaved live walk under a mutating lambda (dissolution.md § "The
    // mutable-array arm"). The pre-order recursion still fuses the innermost single
    // op of a declined mutable chain.
    let mutable_base = base == BaseKind::Mutable;
    if mutable_base && (terminal != TerminalOp::Collect || kinds.len() != 1) {
        return None;
    }
    if ops >= 2 && !all_silent {
        return None;
    }
    Some(ChainPlan {
        terminal,
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

/// Consume a validated chain, returning its **terminal** (Collect, the fold
/// combinator, the count tally, or which search is answered), its per-element
/// `map`/`filter` **stages**
/// (`(kind, param, body)`) in **application order** (innermost op first), and the base
/// collection expression. `plan.terminal` and `plan.kinds` are the chain's shape in
/// outer→inner order (from `validate_chain`); the terminal is peeled first (it wraps
/// the pipeline), then the map/filter ops. Each op's function is resolved by
/// `FnResolver::take_parts` — moved (a lambda literal) or cloned fresh (a named
/// template). Validation guarantees the structure, so every destructuring is total.
///
/// A `count`'s or a search's predicate is returned as an extra **guard stage**
/// appended after the reversal, so it runs last — the outermost op, applied to
/// whatever the inner pipeline threaded through. The stage a `count` appends keeps
/// what its predicate admits; an `all?` appends the one that keeps what its
/// predicate rejects (docs/impl/dissolution.md § "Count — the terminal that is a
/// guard plus a tally", § "Search — the terminal that stops early").
pub(super) fn take_chain(
    mut expr: Hir,
    plan: ChainPlan,
    arena: &mut BindingArena,
    fns: &FnResolver,
) -> (Terminal, Vec<(StageKind, Binding, Hir)>, Hir) {
    // The terminal's own predicate, held aside until the map/filter stages are in
    // application order — it is the outermost op, so it goes last.
    let mut terminal_guard: Option<(StageKind, Binding, Hir)> = None;
    let terminal = match plan.terminal {
        TerminalOp::Fold => {
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
        }
        TerminalOp::Count => {
            let HirKind::Call { args, .. } = expr.kind else {
                unreachable!("validate_chain proved a count call");
            };
            let mut it = args.into_iter();
            let lam = it.next().expect("count has 2 args").expr;
            let coll = it.next().expect("count has 2 args").expr;
            let (params, body) = fns.take_parts(lam, arena);
            expr = coll;
            terminal_guard = Some((StageKind::Keep, params[0], body));
            Terminal::Count
        }
        TerminalOp::Search(search) => {
            let HirKind::Call { args, .. } = expr.kind else {
                unreachable!("validate_chain proved a search call");
            };
            let mut it = args.into_iter();
            let lam = it.next().expect("a search has 2 args").expr;
            let coll = it.next().expect("a search has 2 args").expr;
            let (params, body) = fns.take_parts(lam, arena);
            expr = coll;
            terminal_guard = Some((search.guard(), params[0], body));
            Terminal::Search(search)
        }
        // The mutable-array arm: a mutable `@array` base returns the accumulator
        // unfrozen (validate_chain proves a mutable base is a lone map/filter, so
        // it is always a Collect, never paired with a scalar terminal).
        TerminalOp::Collect => Terminal::Collect {
            unfrozen: plan.mutable_base,
        },
    };

    let mut stages = Vec::with_capacity(plan.kinds.len() + 1);
    for hof in plan.kinds {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a HOF call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("HOF has 2 args").expr;
        let coll = it.next().expect("HOF has 2 args").expr;
        let (params, body) = fns.take_parts(lam, arena);
        stages.push((hof.stage(), params[0], body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    stages.reverse();
    if let Some(guard) = terminal_guard {
        stages.push(guard);
    }
    (terminal, stages, expr)
}
