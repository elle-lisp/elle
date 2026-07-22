//! HOF-chain loop fusion — the first closure dissolution (docs/impl/dissolution.md).
//!
//! At a call `(map f xs)` / `(filter p xs)` where `xs` is a statically-proven
//! immutable array and the lambda is a non-capturing single-parameter one written
//! at the call site, this rewrites the cross-unit stdlib dispatch to the
//! index-walk loop that op's own array arm runs (`src/stdlib.lisp`) — but with the
//! lambda body **spliced inline** rather than called through a closure value. The
//! closure ceases to exist: no per-element closure allocation, no indirect call.
//! `map` pushes each transform's result; `filter` pushes the element itself under
//! an `if` guard. A same-HOF composition — `(map g (map f xs))` or
//! `(filter q (filter p xs))` — fuses to a **single** loop (the transforms nest,
//! or the guards nest), so the intermediate array the inner op would have
//! allocated never exists. A mixed `map`/`filter` chain fuses its inner
//! homogeneous run only (see `validate_chain`); mixing them in one loop is a later
//! widening.
//!
//! ## Why this shape, here
//!
//! The pass emits *surface* HIR — plain `while`/`push`/`freeze` (plus `if` for
//! `filter`), the same shape the stdlib op's body has before functionalization —
//! and runs in `regularize` (`src/hir/regularize.rs`) **before** `functionalize`.
//! So every downstream pass consumes the fused loop exactly as it consumes the
//! op's own body: the `while` becomes a `loop`/`recur`, `push` monomorphizes to
//! `%push-array-mut` on the proven `@array` accumulator, region inference frees
//! the accumulator by subtree drop. The pass never hand-builds a `loop`/`recur`
//! or a capture cell.
//!
//! It mirrors the container-dispatch monomorphization (`monomorphize.rs`):
//! recognize a proven-type call across the compile-unit boundary (the callee is
//! `is_primitive` — a `bind_primitives` stdlib export — and named `map`/`filter`;
//! a user redefinition shadows it with a non-primitive binding and is left alone)
//! and collapse it to the direct form the proof selects.
//!
//! ## Legality
//!
//! Fusion preserves the program's value. A single op also preserves the exact
//! per-element evaluation order (the loop applies the lambda left to right,
//! identically to the stdlib op), so it needs no purity gate. A **composition**
//! interleaves the per-element work (`f x0; g …; f x1; g …`) rather than running
//! all of the first op then all of the second — a reorder observable only through
//! sequencing effects — so each lambda body in a chain of length ≥ 2 must be free
//! of them (`reorder_safe`): no yield/I/O/emit/FFI/halt (a non-capturing lambda's
//! only cross-element channel is such an effect). `SIG_ERROR` is permitted; see
//! `reorder_safe`.

use super::prune::concrete_init_keywords;
use super::unwrap_callee_binding;
use crate::hir::arena::{BindingArena, BindingScope};
use crate::hir::binding::Binding;
use crate::hir::expr::{CallArg, Hir, HirKind};
use crate::primitives::def::RetType;
use crate::signals::{Signal, SIG_ERROR};
use crate::symbol::SymbolTable;
use rustc_hash::FxHashMap;
use std::collections::HashMap;

/// The stdlib/primitive ops the fused loop is built from, resolved once to this
/// unit's bindings by name (every one is an `is_primitive` global bound by
/// `bind_primitives`). If any is absent — an impossible stdlib, but cheap to
/// guard — fusion declines and every `map` call is left intact.
struct Ops {
    /// `(@array)` — the fresh mutable accumulator.
    at_array: Binding,
    /// `(length coll)`.
    length: Binding,
    /// `(get coll i)`.
    get: Binding,
    /// `(push acc elem)` — monomorphizes to `%push-array-mut` on the `@array` acc.
    push: Binding,
    /// `(freeze acc)` — the immutable result.
    freeze: Binding,
    /// `(< i len)`.
    lt: Binding,
    /// `(+ i 1)`.
    add: Binding,
}

impl Ops {
    fn resolve(arena: &BindingArena, symbol_names: &HashMap<u32, String>) -> Option<Ops> {
        // Name → the first `is_primitive` binding for it (each global is bound
        // once by `bind_primitives`, so first-wins is exact).
        let mut prim: HashMap<&str, Binding> = HashMap::new();
        for i in 0..arena.len() as u32 {
            let b = Binding(i);
            let bi = arena.get(b);
            if bi.is_primitive {
                if let Some(name) = symbol_names.get(&bi.name.0) {
                    prim.entry(name.as_str()).or_insert(b);
                }
            }
        }
        let find = |n: &str| prim.get(n).copied();
        Some(Ops {
            at_array: find("@array")?,
            length: find("length")?,
            get: find("get")?,
            push: find("push")?,
            freeze: find("freeze")?,
            lt: find("<")?,
            add: find("+")?,
        })
    }
}

/// Fuse every qualifying `map` chain into an inlined index-walk loop. Runs on
/// surface HIR, before functionalize (see the module doc).
pub(crate) fn fuse_map_chains(hir: &mut Hir, arena: &mut BindingArena, symbols: &SymbolTable) {
    let symbol_names = symbols.all_names();
    let Some(ops) = Ops::resolve(arena, &symbol_names) else {
        return;
    };
    // The sound `binding → type-of keyword` proof dead-arm pruning already
    // computes (`prune::concrete_init_keywords`). A `map`'s base collection may be
    // a `Var` alias of an immutable array, not only a call-site literal; this map
    // is what proves the alias `array`. Built once over the pre-rewrite tree — the
    // base-var bindings live in enclosing `let`s that fusion never mutates, so the
    // proof stays valid as inner map calls collapse.
    let bases = concrete_init_keywords(hir, arena, &symbol_names);
    rewrite(hir, arena, &symbol_names, &ops, &bases, false);
}

/// Pre-order walk: try to fuse a HOF chain rooted at `hir` (consuming the whole
/// chain, including its inner HOF calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested HOFs in the spliced lambda
/// bodies or the base array's elements).
///
/// `suppress` blocks fusion at this node (but not the recursion). It is set for
/// any subtree positioned **after a surviving lambda-literal argument** in the
/// same function body: a fused loop lowers to a `block`/`loop`, and a lambda
/// literal that is allocated *before* a loop in the same function body and then
/// called is mis-lowered to a phantom extra parameter (an arity error at the
/// call). The fusion consumes its own lambda, so a single/composed HOF in
/// statement, binding, or native-argument position is always safe; the unsafe
/// shape is a fused loop landing beside a lambda the fusion did NOT consume —
/// e.g. the mixed `(map f (filter p xs))`, where `map`'s `f` survives and the
/// inner `filter` would fuse. Suppression propagates through nested expressions
/// and **resets at a `Lambda` boundary** (a nested lambda body is a separate
/// lowered function, so an outer sibling lambda cannot clobber a loop inside it).
fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbol_names: &HashMap<u32, String>,
    ops: &Ops,
    bases: &FxHashMap<Binding, &'static str>,
    suppress: bool,
) {
    if !suppress {
        if let Some((hof, len)) = validate_chain(hir, arena, symbol_names, bases) {
            let sig = hir.signal;
            let span = hir.span.clone();
            let owned = std::mem::replace(hir, Hir::error(span.clone()));
            let (transforms, base) = take_chain(owned, len);
            *hir = build_loop(hof, transforms, base, arena, ops, sig, span);
        }
    }
    match &mut hir.kind {
        // A lambda body is a separate lowered function: reset the unsafe context.
        HirKind::Lambda { body, .. } => rewrite(body, arena, symbol_names, ops, bases, false),
        // An argument after a lambda-literal sibling is unsafe (see above); the
        // suppression carries into that argument's whole subtree.
        HirKind::Call { func, args, .. } => {
            rewrite(func, arena, symbol_names, ops, bases, suppress);
            let mut after_lambda = false;
            for a in args.iter_mut() {
                let is_lambda = matches!(a.expr.kind, HirKind::Lambda { .. });
                rewrite(
                    &mut a.expr,
                    arena,
                    symbol_names,
                    ops,
                    bases,
                    suppress || after_lambda,
                );
                after_lambda |= is_lambda;
            }
        }
        _ => hir.for_each_child_mut(|c| rewrite(c, arena, symbol_names, ops, bases, suppress)),
    }
}

/// The higher-order collection op a fused chain is built from. Both take
/// `(lambda, coll)` and run the same `(get`/`push`/`freeze)` index-walk over
/// `coll`'s array arm; they differ only in the per-element loop body
/// (`build_loop`): `Map` pushes the lambda's *result*, `Filter` pushes the
/// element itself under an `if` guard.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hof {
    Map,
    Filter,
}

impl Hof {
    /// The canonical stdlib name this op is recognized by.
    fn from_name(name: &str) -> Option<Hof> {
        match name {
            "map" => Some(Hof::Map),
            "filter" => Some(Hof::Filter),
            _ => None,
        }
    }
}

/// A recognized `(map <lambda> …)` / `(filter <lambda> …)` call: the HOF kind,
/// the lambda argument, and the collection argument (both borrowed). `None` when
/// `hir` is not a call to the canonical stdlib `map`/`filter` with exactly two
/// non-spliced arguments (a user redefinition shadows the name with a
/// non-primitive binding and is excluded).
fn fusable_hof_parts<'a>(
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

/// The parameter binding and body of a lambda that qualifies for inlining, or
/// `None`. A qualifying `f` is a lambda literal with exactly one fixed parameter
/// (no rest), no captures (its body references only the parameter and globals,
/// so splicing at the call site is always in scope), an unmutated parameter, and
/// **no nested lambda** in its body (so retyping the parameter to a plain local
/// cannot disturb a capture of it). These bounds keep the splice a straight
/// `(let [param elem] body)` with no substitution or cell reasoning.
fn qualifies_lambda<'a>(lam: &'a Hir, arena: &BindingArena) -> Option<(Binding, &'a Hir)> {
    let HirKind::Lambda {
        params,
        rest_param,
        captures,
        body,
        ..
    } = &lam.kind
    else {
        return None;
    };
    if rest_param.is_some() || params.len() != 1 || !captures.is_empty() {
        return None;
    }
    let param = params[0];
    if arena.get(param).is_mutated || body_disqualifies(body) {
        return None;
    }
    Some((param, body))
}

/// Does a lambda body disqualify it from inlining? Two structural hazards, both
/// detected in one walk:
///
/// - **A nested lambda** — retyping the parameter to a plain local (the splice)
///   could disturb a capture of it, and a per-element closure is not the kernel
///   this fusion targets.
/// - **A call-position `%`-intrinsic or `(numeric!)`** — a raw intrinsic carries
///   an operand proof obligation (`docs/intrinsics.md`) that the *lambda* context
///   discharges: `(numeric!)` floors the lambda's parameter at Number, which is
///   what proves a `(%add x 1)` body. Inlining de-lambdas the parameter, binding
///   it to `(get coll i)` (whose element type is not proven at this stage), so
///   the proof vanishes and the contract check would reject the spliced form.
///   Ordinary numeric kernels use the stdlib wrappers (`+`/`*`), which are plain
///   calls here and validate dynamically — so only hand-written raw-intrinsic
///   bodies are declined, and they stay plain `map` calls.
fn body_disqualifies(hir: &Hir) -> bool {
    if matches!(hir.kind, HirKind::Lambda { .. } | HirKind::Intrinsic { .. }) {
        return true;
    }
    let mut found = false;
    hir.for_each_child(|c| found |= body_disqualifies(c));
    found
}

/// Validate that `hir` is a fusable **homogeneous** HOF chain and return its kind
/// and length (the number of nested ops). The chain bottoms out at a proven
/// immutable array; every op is the same HOF (`map`-of-`map` or
/// `filter`-of-`filter`); every lambda qualifies (`qualifies_lambda`); and for a
/// composition (length ≥ 2) every lambda body is `reorder_safe` (the reordering
/// gate — see the module doc).
///
/// A kind change ends the chain: the walk stops and the differing call becomes the
/// base candidate, which is not a proven immutable array, so a mixed
/// `(map f (filter p xs))` declines at the outer op. The inner homogeneous run
/// still fuses on the pre-order recursion (`rewrite`).
fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    bases: &FxHashMap<Binding, &'static str>,
) -> Option<(Hof, usize)> {
    let mut len = 0usize;
    let mut all_silent = true;
    let mut kind: Option<Hof> = None;
    let mut cur = hir;
    while let Some((hof, lam, coll)) = fusable_hof_parts(cur, arena, symbol_names) {
        if kind.is_some_and(|k| k != hof) {
            break;
        }
        let (_, body) = qualifies_lambda(lam, arena)?;
        all_silent &= reorder_safe(body.signal);
        kind = Some(hof);
        len += 1;
        cur = coll;
    }
    let hof = kind?;
    if !coll_is_immutable_array(cur, arena, symbol_names, bases) {
        return None;
    }
    if len >= 2 && !all_silent {
        return None;
    }
    Some((hof, len))
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
fn reorder_safe(sig: Signal) -> bool {
    sig.bits.subtract(SIG_ERROR).is_empty() && sig.propagates == 0
}

/// Is `expr` a statically-proven immutable array? Two proven forms:
///
/// - **A call-site immutable-array producer** — a call to a primitive whose
///   declared `RetType` is `Array`: an array literal (`[ … ]` → the `array`
///   primitive) or any other `RetType::Array` native (`->array`, …).
/// - **A `Var` alias of one** — a binding whose initializer resolves through the
///   shared init proof (`bases`, built by `prune::concrete_init_keywords`) to the
///   `array` keyword, following immutable/unmutated/single-init alias chains to a
///   fixpoint. This is the SAME proof dead-arm pruning trusts to delete a match
///   arm, so accepting it here carries the identical soundness guarantee; a
///   mutable `@array` base resolves to `@array` (not `array`) and is declined.
fn coll_is_immutable_array(
    expr: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    bases: &FxHashMap<Binding, &'static str>,
) -> bool {
    if let HirKind::Var(b) = &expr.kind {
        return bases.get(b) == Some(&"array");
    }
    let HirKind::Call { func, .. } = &expr.kind else {
        return false;
    };
    let Some(callee) = unwrap_callee_binding(func) else {
        return false;
    };
    let bi = arena.get(callee);
    if !bi.is_primitive || !bi.is_immutable || bi.is_mutated {
        return false;
    }
    let Some(name) = symbol_names.get(&bi.name.0) else {
        return false;
    };
    crate::primitives::registration::def_by_name(name).map(|d| d.ret) == Some(RetType::Array)
}

/// Consume a validated chain of `len` nested HOF calls, returning the per-element
/// lambdas (`(param, body)`) in **application order** (innermost op's lambda
/// first) and the base collection expression. For `map` these are transforms; for
/// `filter` they are predicates. Validation (`validate_chain`) guarantees the
/// structure, so the destructuring is total.
fn take_chain(mut expr: Hir, len: usize) -> (Vec<(Binding, Hir)>, Hir) {
    let mut lambdas = Vec::with_capacity(len);
    for _ in 0..len {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a HOF call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("HOF has 2 args").expr;
        let coll = it.next().expect("HOF has 2 args").expr;
        let HirKind::Lambda { params, body, .. } = lam.kind else {
            unreachable!("validate_chain proved a lambda");
        };
        lambdas.push((params[0], *body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    lambdas.reverse();
    (lambdas, expr)
}

/// Build the fused index-walk loop from the per-element lambdas and base
/// collection. The `(get`/`push`/`freeze)` scaffold is shared; only the loop body
/// differs by HOF.
///
/// `map` pushes each transform's result:
/// ```text
/// (while (< i len)
///   (push acc (let [p0 (get coll i)] B0 … as nested lets …))
///   (assign i (+ i 1)))
/// ```
///
/// `filter` binds the element once and pushes it under a guard per predicate
/// (nested innermost-first, so the first-applied filter is the outer `if`):
/// ```text
/// (while (< i len)
///   (let [item (get coll i)]
///     (if (let [p item] P0) (if (let [q item] P1) (push acc item) nil) nil))
///   (assign i (+ i 1)))
/// ```
///
/// The synthesized helper calls and `if`/`let` scaffolding carry `sig` (the
/// original call's signal, a sound upper bound over every op in the stdlib op's
/// body); the spliced lambda bodies keep their own signals. Bottom-up
/// re-propagation (`hir/narrow.rs`) then rebuilds the fused form's signal from
/// these leaves without under-reporting.
fn build_loop(
    hof: Hof,
    lambdas: Vec<(Binding, Hir)>,
    base: Hir,
    arena: &mut BindingArena,
    ops: &Ops,
    sig: Signal,
    span: crate::syntax::Span,
) -> Hir {
    let local = |arena: &mut BindingArena| {
        let b = arena.gensym();
        arena.get_mut(b).is_immutable = true;
        b
    };
    let coll_b = local(arena);
    let len_b = local(arena);
    let acc_b = local(arena);
    let i_b = arena.gensym();
    arena.get_mut(i_b).is_mutated = true; // the loop induction variable

    let node = |kind: HirKind| Hir::new(kind, span.clone(), sig);
    let var = |b: Binding| Hir::new(HirKind::Var(b), span.clone(), Signal::silent());
    let int = |n: i64| Hir::new(HirKind::Int(n), span.clone(), Signal::silent());
    let nil = || Hir::new(HirKind::Nil, span.clone(), Signal::silent());
    let call = |f: Binding, args: Vec<Hir>| {
        Hir::new(
            HirKind::Call {
                func: Box::new(Hir::new(HirKind::Var(f), span.clone(), Signal::silent())),
                args: args
                    .into_iter()
                    .map(|expr| CallArg {
                        expr,
                        spliced: false,
                    })
                    .collect(),
                is_tail: false,
            },
            span.clone(),
            sig,
        )
    };
    // Each spliced lambda parameter — no longer a lambda parameter, since the
    // lambda is consumed — is retyped to a plain immutable local so the lowerer
    // gives it a local slot, not an argument slot.
    let to_local = |arena: &mut BindingArena, param: Binding| {
        let pi = arena.get_mut(param);
        pi.scope = BindingScope::Local;
        pi.is_immutable = true;
    };

    let body_stmt = match hof {
        // (push acc (let [p0 (get coll i)] B0 … nested innermost-first …))
        Hof::Map => {
            let mut elem = call(ops.get, vec![var(coll_b), var(i_b)]);
            for (param, body) in lambdas {
                to_local(arena, param);
                elem = node(HirKind::Let {
                    bindings: vec![(param, elem)],
                    body: Box::new(body),
                });
            }
            call(ops.push, vec![var(acc_b), elem])
        }
        // (let [item (get coll i)]
        //   (if (let [p item] P0) … (push acc item) …))
        // The element is bound once and pushed only when every predicate passes;
        // guards nest innermost-first (the first-applied filter is the outer `if`),
        // so fold the predicates in reverse application order around the push.
        Hof::Filter => {
            let item_b = local(arena);
            let mut guarded = call(ops.push, vec![var(acc_b), var(item_b)]);
            for (param, pred) in lambdas.into_iter().rev() {
                to_local(arena, param);
                let cond = node(HirKind::Let {
                    bindings: vec![(param, var(item_b))],
                    body: Box::new(pred),
                });
                guarded = node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(guarded),
                    else_branch: Box::new(nil()),
                });
            }
            node(HirKind::Let {
                bindings: vec![(item_b, call(ops.get, vec![var(coll_b), var(i_b)]))],
                body: Box::new(guarded),
            })
        }
    };
    let incr = node(HirKind::Assign {
        target: i_b,
        value: Box::new(call(ops.add, vec![var(i_b), int(1)])),
    });
    let while_loop = node(HirKind::While {
        cond: Box::new(call(ops.lt, vec![var(i_b), var(len_b)])),
        body: Box::new(node(HirKind::Begin(vec![body_stmt, incr]))),
    });
    let define_i = node(HirKind::Define {
        binding: i_b,
        value: Box::new(int(0)),
    });
    let freeze = call(ops.freeze, vec![var(acc_b)]);
    let acc_body = node(HirKind::Begin(vec![define_i, while_loop, freeze]));

    let acc_let = node(HirKind::Let {
        bindings: vec![(acc_b, call(ops.at_array, vec![]))],
        body: Box::new(acc_body),
    });
    let len_let = node(HirKind::Let {
        bindings: vec![(len_b, call(ops.length, vec![var(coll_b)]))],
        body: Box::new(acc_let),
    });
    node(HirKind::Let {
        bindings: vec![(coll_b, base)],
        body: Box::new(len_let),
    })
}

#[cfg(test)]
mod tests {
    use crate::hir::arena::BindingArena;
    use crate::hir::expr::{Hir, HirKind};
    use std::collections::HashMap;

    /// Compile a source form to functionalized HIR against a full stdlib.
    fn compile(src: &str) -> (Hir, BindingArena, HashMap<u32, String>) {
        let mut rt = crate::runtime::Runtime::new();
        let (_vm, symbols, cctx) = rt.parts();
        crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>").expect("compile")
    }

    /// Names of every call callee (through the ANF/`Var` wrappers) in the tree.
    fn callee_names(
        h: &Hir,
        arena: &BindingArena,
        names: &HashMap<u32, String>,
        out: &mut Vec<String>,
    ) {
        if let HirKind::Call { func, .. } = &h.kind {
            if let Some(b) = super::unwrap_callee_binding(func) {
                if let Some(n) = names.get(&arena.get(b).name.0) {
                    out.push(n.clone());
                }
            }
        }
        h.for_each_child(|c| callee_names(c, arena, names, out));
    }

    fn callees(h: &Hir, arena: &BindingArena, names: &HashMap<u32, String>) -> Vec<String> {
        let mut out = Vec::new();
        callee_names(h, arena, names, &mut out);
        out
    }

    /// Count the lambda nodes remaining in the tree — the closure(s) fusion
    /// dissolves.
    fn count_lambdas(h: &Hir) -> usize {
        let mut n = usize::from(matches!(h.kind, HirKind::Lambda { .. }));
        h.for_each_child(|c| n += count_lambdas(c));
        n
    }

    /// Count the `if` nodes — a fused `filter` emits one guarded push per
    /// predicate stage; a fused `map` emits none.
    fn count_ifs(h: &Hir) -> usize {
        let mut n = usize::from(matches!(h.kind, HirKind::If { .. }));
        h.for_each_child(|c| n += count_ifs(c));
        n
    }

    /// The gauge (docs/impl/dissolution.md § "The gauge"): `(map f xs)` over a
    /// proven immutable array with an inline lambda `f` dissolves — the `map`
    /// dispatch is gone, no closure survives, and `f`'s body op (`*`) runs inline
    /// in the loop. Fails before fusion lands: the `map` call and the `(fn [x] …)`
    /// closure are both present.
    #[test]
    fn single_map_dissolves_the_closure_and_dispatch() {
        let (hir, arena, names) = compile("(map (fn [x] (* x 2)) [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "the `map` dispatch must be gone; callees were {cs:?}",
        );
        assert_eq!(
            count_lambdas(&hir),
            0,
            "no closure may survive — `f`'s body is spliced inline",
        );
        assert!(
            cs.iter().any(|n| n == "*"),
            "`f`'s body op `*` must run inline in the loop; callees were {cs:?}",
        );
        // The loop `map`'s array arm runs: one fresh accumulator, filled and frozen.
        assert!(
            cs.iter().any(|n| n == "freeze"),
            "the fused loop must freeze one accumulator; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "exactly one accumulator; callees were {cs:?}",
        );
    }

    /// A composition `(map g (map f xs))` fuses to a SINGLE loop: no `map`, both
    /// transform ops (`*` and `+`) inline, and — the intermediate collection is
    /// gone — exactly one accumulator. Fails before fusion: two `map` calls, two
    /// closures, and (were only the single case built) two accumulators.
    #[test]
    fn composed_maps_fuse_to_one_loop() {
        let (hir, arena, names) = compile("(map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "both `map` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "+"),
            "both transforms must inline; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
        );
    }

    /// Safety: a user redefinition of `map` shadows the stdlib binding with a
    /// non-primitive one, so it is never rewritten (`fusable_map_parts` gates on
    /// `is_primitive`). The user's `map` call survives.
    #[test]
    fn user_shadowed_map_is_not_fused() {
        let (hir, arena, names) = compile("(defn map [f xs] xs) (map (fn [x] x) [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a user `map` must not be rewritten; callees were {cs:?}",
        );
    }

    /// Safety: a capturing lambda is left alone (its body references a free
    /// variable, so it is not the non-capturing kernel the gate admits). The
    /// `map` call survives.
    #[test]
    fn capturing_lambda_is_not_fused() {
        let (hir, arena, names) = compile("(let [k 10] (map (fn [x] (+ x k)) [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a capturing lambda must not fuse; callees were {cs:?}",
        );
        assert!(count_lambdas(&hir) >= 1, "the closure must survive");
    }

    /// Safety: a `map` over a value that is not a proven immutable array (here a
    /// runtime parameter) is left alone — fusion fires only on the array arm the
    /// type proof selects.
    #[test]
    fn map_over_unproven_collection_is_not_fused() {
        let (hir, arena, names) = compile("(defn f [xs] (map (fn [x] (* x 2)) xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "an unproven collection must not fuse; callees were {cs:?}",
        );
    }

    /// A single `filter` dissolves to the guarded-push index-walk: the `filter`
    /// dispatch is gone, no closure survives, the predicate op (`>` — deliberately
    /// absent from the loop scaffold, which uses only `<`/`+`) runs inline, and the
    /// loop body is an `if` (the conditional push) over one frozen accumulator.
    /// Fails before filter fusion lands: the `filter` call and the `(fn …)` closure
    /// are both present and there is no synthesized `if`.
    #[test]
    fn single_filter_dissolves_to_guarded_push() {
        let (hir, arena, names) = compile("(filter (fn [x] (> x 2)) [1 2 3 4])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "the `filter` dispatch must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == ">"),
            "the predicate op `>` must run inline; callees were {cs:?}",
        );
        assert!(
            count_ifs(&hir) >= 1,
            "the fused filter must emit a guarded push (an `if`)",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "exactly one accumulator; callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == "freeze"),
            "the fused loop must freeze one accumulator; callees were {cs:?}",
        );
    }

    /// A `filter`-of-`filter` fuses to a SINGLE loop with the guards nested: no
    /// `filter`, both predicate ops (`even?` and `integer?`) inline, one
    /// accumulator, and two `if`s (one per predicate). The predicates must be
    /// reorder-safe for a length-2 composition to fuse (the reordering gate — a
    /// variadic comparison like `>` routes through `apply` and is NOT reorder-safe,
    /// so it fuses as a single filter but declines composition; `even?`/`integer?`
    /// carry only `SIG_ERROR`). Fails before fusion: two `filter` calls, two closures.
    #[test]
    fn composed_filters_fuse_to_one_loop() {
        let (hir, arena, names) =
            compile("(filter (fn [y] (even? y)) (filter (fn [x] (integer? x)) [1 2 3 4 5]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "both `filter` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "even?") && cs.iter().any(|n| n == "integer?"),
            "both predicates must inline; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "one loop, one accumulator; callees were {cs:?}",
        );
        assert!(
            count_ifs(&hir) >= 2,
            "each predicate stage emits its own guard `if`",
        );
    }

    /// A `filter` over a `Var`-bound immutable array fuses — the base-alias proof
    /// and the guarded-push shape compose.
    #[test]
    fn filter_over_var_bound_immutable_array_fuses() {
        let (hir, arena, names) = compile("(let [xs [1 2 3 4]] (filter (fn [x] (> x 2)) xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "a Var-bound base must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
    }

    /// Scope boundary: a MIXED `(map f (filter p xs))` fuses NOTHING. The outer
    /// `map` is homogeneous-only (declines over a filter), and the inner `filter`
    /// is suppressed because it sits after `map`'s surviving lambda argument `f` —
    /// fusing it there would land a `loop`/`block` beside a lambda literal in the
    /// same function body, a shape the lowerer mis-compiles to a phantom-arity
    /// closure (`rewrite`'s `suppress`). So both HOFs are left as plain calls, and
    /// the mixed form computes correctly un-fused (`dissolution-filter-fuse.lisp`).
    /// Fusing them into one loop is a later widening.
    #[test]
    fn mixed_map_of_filter_is_not_fused() {
        let (hir, arena, names) =
            compile("(map (fn [x] (* x 2)) (filter (fn [w] (> w 1)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "the outer `map` must not fuse over a filter; callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == "filter"),
            "the inner `filter` must be suppressed beside the surviving lambda; \
             callees were {cs:?}",
        );
    }

    /// Safety: a capturing predicate is left alone (it references a free variable,
    /// so it is not the non-capturing kernel the gate admits). The `filter` call
    /// survives.
    #[test]
    fn capturing_predicate_is_not_fused() {
        let (hir, arena, names) = compile("(let [k 2] (filter (fn [x] (> x k)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "filter"),
            "a capturing predicate must not fuse; callees were {cs:?}",
        );
        assert!(count_lambdas(&hir) >= 1, "the closure must survive");
    }

    /// A `map` over a `Var` whose initializer is a proven immutable array fuses:
    /// the base need not be written as a literal at the call site. The proof is
    /// the same binding→keyword map dead-arm pruning builds (`prune::classify_init`),
    /// so `(let [xs [1 2 3]] (map f xs))` dissolves exactly as `(map f [1 2 3])`
    /// does. Fails before the Var-base widening lands: the `map` call and closure
    /// both survive because the base is a `Var`, not a literal `array` call.
    #[test]
    fn map_over_var_bound_immutable_array_fuses() {
        let (hir, arena, names) = compile("(let [xs [1 2 3]] (map (fn [x] (* x 2)) xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "a Var-bound immutable array must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "*"),
            "`f`'s body op must inline; callees were {cs:?}",
        );
    }

    /// The alias proof follows a chain to a fixpoint (`prune::resolve`): `ys`
    /// aliases `xs` aliases the literal, so `(map f ys)` still fuses.
    #[test]
    fn map_over_aliased_var_immutable_array_fuses() {
        let (hir, arena, names) =
            compile("(let [xs [1 2 3]] (let [ys xs] (map (fn [x] (* x 2)) ys)))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "an aliased Var over an immutable array must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    }

    /// Safety: the widening is immutable-only. A `Var` bound to a **mutable**
    /// array (`@[ … ]`, keyword `@array`) is left to the stdlib `map` — its result
    /// aliases the input's mutability, which the general path handles. The proof
    /// map resolves the base to `@array`, not `array`, so fusion declines.
    #[test]
    fn map_over_var_bound_mutable_array_is_not_fused() {
        let (hir, arena, names) = compile("(let [xs @[1 2 3]] (map (fn [x] (* x 2)) xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a mutable `@array` base must not fuse (immutable-only); callees were {cs:?}",
        );
    }

    /// A lambda body with a raw call-position `%`-intrinsic is declined — and this
    /// must be a DECLINE, not a break. `(numeric!)` floors the *lambda parameter*
    /// at Number, which is the sole proof that discharges `(%add x 1)`'s prove-or-
    /// reject obligation (strip `numeric!` and even the un-fused lambda fails to
    /// compile). Inlining dissolves the lambda, so that param-scoped floor cannot
    /// survive; fusing anyway would leave `(%add (get coll i) 1)` with an unproven
    /// operand and turn a compiling program into a compile error. So the pass MUST
    /// leave the `map` call intact here — the boundary is a correctness
    /// requirement, not a missed optimization. (Fusing this shape is headroom:
    /// it needs element-type inference through `get` to reconstruct the proof the
    /// vanished lambda provided.) The value-side proof that the declined form still
    /// computes correctly is `tests/elle/dissolution-map-fuse.lisp`.
    #[test]
    fn intrinsic_body_map_is_declined_not_broken() {
        // Compiles (the whole point — fusing would make it uncompilable) …
        let (hir, arena, names) = compile("(map (fn [x] (numeric!) (%add x 1)) [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        // … and is left as a plain `map` call, the closure intact.
        assert!(
            cs.iter().any(|n| n == "map"),
            "a raw-intrinsic body must not fuse (its param proof cannot survive \
             de-lambda'ing); callees were {cs:?}",
        );
        assert!(count_lambdas(&hir) >= 1, "the closure must survive");
    }
}
