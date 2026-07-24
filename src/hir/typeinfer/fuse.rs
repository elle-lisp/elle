//! HOF-chain loop fusion — the first closure dissolution (docs/impl/dissolution.md).
//!
//! At a call `(map f xs)` / `(filter p xs)` where `xs` is a statically-proven
//! immutable array and the lambda is a non-capturing single-parameter one written
//! at the call site, this rewrites the cross-unit stdlib dispatch to the
//! index-walk loop that op's own array arm runs (`src/stdlib.lisp`) — but with the
//! lambda body **spliced inline** rather than called through a closure value. The
//! closure ceases to exist: no per-element closure allocation, no indirect call.
//! `map` pushes each transform's result; `filter` pushes the element itself under
//! an `if` guard. `fold`/`reduce` (`(fold f init xs)`, `f` called `(f acc elem)`)
//! is the chain's optional outermost **terminal**: a scalar accumulator seeded by
//! `init` and updated one left-fold step per element, so there is no `@array` and
//! no `freeze` — the result is the accumulator's final value. A composition —
//! `(map g (map f xs))`, `(filter q (filter p xs))`, any mix like `(map f (filter
//! p xs))`, or a fold over a map/filter prefix like `(fold f init (map g xs))` —
//! fuses to a **single** loop through one unified transform/guard pipeline
//! (`build_loop`/`Build::element`): each `map`/`filter` op is a *stage* (a `map`
//! transforms the threaded value; a `filter` guards it), the stages nest in
//! application order, and the base case is the terminal (a `push` for a collect, a
//! fold step for a fold). The intermediate array any inner op would have allocated
//! never exists. `map`-only and `filter`-only chains are just the all-transform and
//! all-guard ends of the collect pipeline; a fold reuses the same stages with a
//! scalar terminal — the map-reduce shape, no array at all.
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
use rustc_hash::{FxHashMap, FxHashSet};
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
    // The same-unit function templates: a `Var` naming a non-capturing lambda
    // (a top-level `defn` or a `let`/`def`-bound `fn`) inlines like a literal
    // (docs/impl/dissolution.md § "Named same-unit functions"). Built once over
    // the pre-rewrite tree; each use clones a fresh copy, so the map stays valid
    // as calls collapse.
    let mut templates: FxHashMap<Binding, FnTemplate> = FxHashMap::default();
    collect_inline_fns(hir, arena, &mut templates, &mut FxHashSet::default());
    rewrite(hir, arena, &symbol_names, &ops, &bases, &templates);
}

/// A same-unit function eligible for inlining into a fused HOF: its parameters
/// and body, held as an owned template. Because the definition persists (it stays
/// bound and may be used as a first-class value), its body cannot be moved out;
/// each call site clones this template with fresh bindings and HirIds (see
/// `clone_template`/`clone_fresh`).
struct FnTemplate {
    params: Vec<Binding>,
    body: Hir,
}

/// Walk every `Let`/`Letrec`/`Define` binding (the same forms `prune::collect_inits`
/// visits) and record those bound to an inlineable lambda template. Mirrors the
/// singly-bound/immutable/unmutated discipline of the init-keyword proof.
fn collect_inline_fns(
    hir: &Hir,
    arena: &BindingArena,
    out: &mut FxHashMap<Binding, FnTemplate>,
    seen: &mut FxHashSet<Binding>,
) {
    let mut record = |b: Binding, value: &Hir, out: &mut FxHashMap<Binding, FnTemplate>| {
        // A binding bound more than once has no single stable value — drop it.
        if !seen.insert(b) {
            out.remove(&b);
            return;
        }
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        if let Some(t) = fn_template(value, arena) {
            out.insert(b, t);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, value) in bindings {
                record(*b, value, out);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, out),
        _ => {}
    }
    hir.for_each_child(|c| collect_inline_fns(c, arena, out, seen));
}

/// The inlineable template of a lambda initializer, or `None`. A qualifying lambda
/// is non-capturing, has 1 or 2 fixed parameters (a `map`/`filter` element or a
/// `fold` accumulator+element — the use site checks the exact arity), no rest
/// parameter, unmutated parameters, and a `clone_fresh`-admissible body
/// (`is_inlineable_body` — the pure-expression forms plus `let`, so the clone
/// freshens the parameters and any `let`-bound bindings and nothing else). The body
/// is cloned into the template; each call site re-clones it with fresh bindings.
fn fn_template(value: &Hir, arena: &BindingArena) -> Option<FnTemplate> {
    let HirKind::Lambda {
        params,
        rest_param,
        captures,
        body,
        ..
    } = &value.kind
    else {
        return None;
    };
    if rest_param.is_some() || !captures.is_empty() || params.is_empty() || params.len() > 2 {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || !is_inlineable_body(body) {
        return None;
    }
    Some(FnTemplate {
        params: params.clone(),
        body: (**body).clone(),
    })
}

/// Is a body admissible for the alpha-renaming clone? The whitelist covers the
/// pure-expression forms plus `let`. A pure-expression body freshens the
/// parameters, rewrites their references, and leaves every other `Var` (a global)
/// shared. A `let` additionally introduces bindings of its own — those are
/// freshened too (`clone_fresh`'s `Let` arm re-mints each `let`-bound binding), so
/// a `let` body clones without collision. `letrec` is **not** admitted (its value
/// may reference its own binding — a forward/self reference the sequential rename
/// cannot satisfy — and the recursive cell it builds is the shape fusion avoids); a
/// body with a `loop`/`match` binding or a nested lambda uses a form not listed
/// here and declines: the definition's own bindings are then never duplicated
/// (correct-by-construction). Kept in lockstep with `clone_fresh` — the same
/// variants, one returning `bool`, one rebuilding.
fn is_inlineable_body(h: &Hir) -> bool {
    match &h.kind {
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_) => true,
        HirKind::Let { bindings, body } => {
            bindings.iter().all(|(_, v)| is_inlineable_body(v)) && is_inlineable_body(body)
        }
        HirKind::Call { func, args, .. } => {
            is_inlineable_body(func) && args.iter().all(|a| is_inlineable_body(&a.expr))
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            is_inlineable_body(cond)
                && is_inlineable_body(then_branch)
                && is_inlineable_body(else_branch)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            clauses
                .iter()
                .all(|(c, b)| is_inlineable_body(c) && is_inlineable_body(b))
                && else_branch.as_ref().is_none_or(|e| is_inlineable_body(e))
        }
        HirKind::Begin(v) | HirKind::And(v) | HirKind::Or(v) => v.iter().all(is_inlineable_body),
        _ => false,
    }
}

/// Deep-clone a whitelisted body with **fresh HirIds** (via `Hir::new` — a plain
/// `.clone()` would duplicate the global-counter ids and collide in the region
/// walk's per-id side tables) and **renamed bindings** (`renames`, old → fresh):
/// the parameters (seeded by `clone_template`) plus every `let`-bound binding the
/// body introduces (freshened in the `Let` arm as the clone descends). Every
/// non-renamed `Var` (a global) is left as-is. `renames` is threaded `&mut` so a
/// nested `let` can extend it, and `arena` `&mut` so a `let` binding can mint its
/// fresh id. Returns `None` on any form `is_inlineable_body` rejects — the two are
/// kept in lockstep, so a body that passed collection always clones.
fn clone_fresh(
    h: &Hir,
    renames: &mut FxHashMap<Binding, Binding>,
    arena: &mut BindingArena,
) -> Option<Hir> {
    let kind = match &h.kind {
        HirKind::Nil => HirKind::Nil,
        HirKind::EmptyList => HirKind::EmptyList,
        HirKind::Bool(b) => HirKind::Bool(*b),
        HirKind::Int(n) => HirKind::Int(*n),
        HirKind::Float(f) => HirKind::Float(*f),
        HirKind::String(s) => HirKind::String(s.clone()),
        HirKind::Keyword(s) => HirKind::Keyword(s.clone()),
        HirKind::Var(b) => HirKind::Var(renames.get(b).copied().unwrap_or(*b)),
        // A `let` freshens its own bindings. Each value is cloned under the renames
        // established so far — before its binding is inserted — so a sequential
        // `let`'s later value sees the fresh id of an earlier binding, while a
        // binding's own value never renames to itself (that is `letrec`, excluded).
        // Each fresh binding is faithful to the source's mutability.
        HirKind::Let { bindings, body } => {
            let mut new_bindings = Vec::with_capacity(bindings.len());
            for (b, value) in bindings {
                let value = clone_fresh(value, renames, arena)?;
                let (is_immutable, is_mutated) = {
                    let bi = arena.get(*b);
                    (bi.is_immutable, bi.is_mutated)
                };
                let fresh = arena.gensym();
                let fi = arena.get_mut(fresh);
                fi.is_immutable = is_immutable;
                fi.is_mutated = is_mutated;
                renames.insert(*b, fresh);
                new_bindings.push((fresh, value));
            }
            let body = Box::new(clone_fresh(body, renames, arena)?);
            HirKind::Let {
                bindings: new_bindings,
                body,
            }
        }
        HirKind::Call {
            func,
            args,
            is_tail,
        } => {
            let func = Box::new(clone_fresh(func, renames, arena)?);
            let mut new_args = Vec::with_capacity(args.len());
            for a in args {
                new_args.push(CallArg {
                    expr: clone_fresh(&a.expr, renames, arena)?,
                    spliced: a.spliced,
                });
            }
            HirKind::Call {
                func,
                args: new_args,
                is_tail: *is_tail,
            }
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => HirKind::If {
            cond: Box::new(clone_fresh(cond, renames, arena)?),
            then_branch: Box::new(clone_fresh(then_branch, renames, arena)?),
            else_branch: Box::new(clone_fresh(else_branch, renames, arena)?),
        },
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            let mut cs = Vec::with_capacity(clauses.len());
            for (c, b) in clauses {
                cs.push((
                    clone_fresh(c, renames, arena)?,
                    clone_fresh(b, renames, arena)?,
                ));
            }
            let eb = match else_branch {
                Some(e) => Some(Box::new(clone_fresh(e, renames, arena)?)),
                None => None,
            };
            HirKind::Cond {
                clauses: cs,
                else_branch: eb,
            }
        }
        HirKind::Begin(v) => HirKind::Begin(clone_vec(v, renames, arena)?),
        HirKind::And(v) => HirKind::And(clone_vec(v, renames, arena)?),
        HirKind::Or(v) => HirKind::Or(clone_vec(v, renames, arena)?),
        _ => return None,
    };
    Some(Hir::new(kind, h.span.clone(), h.signal))
}

/// Clone a slice of whitelisted bodies (a `begin`/`and`/`or` operand list) with the
/// same fresh-id/rename discipline as `clone_fresh`. `None` if any element rejects.
fn clone_vec(
    v: &[Hir],
    renames: &mut FxHashMap<Binding, Binding>,
    arena: &mut BindingArena,
) -> Option<Vec<Hir>> {
    v.iter().map(|c| clone_fresh(c, renames, arena)).collect()
}

/// Clone a function template with fresh parameter bindings (minted via `gensym`,
/// typed immutable-local) and a fresh-id body. Returns the fresh parameters and
/// the cloned body, ready to splice like a moved-out lambda's.
fn clone_template(t: &FnTemplate, arena: &mut BindingArena) -> (Vec<Binding>, Hir) {
    let mut renames: FxHashMap<Binding, Binding> = FxHashMap::default();
    let mut params = Vec::with_capacity(t.params.len());
    for &p in &t.params {
        let fresh = arena.gensym();
        arena.get_mut(fresh).is_immutable = true;
        renames.insert(p, fresh);
        params.push(fresh);
    }
    let body = clone_fresh(&t.body, &mut renames, arena)
        .expect("collect_inline_fns proved the body inlineable");
    (params, body)
}

/// Pre-order walk: try to fuse a HOF chain rooted at `hir` (consuming the whole
/// chain, including its inner HOF calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested HOFs in the spliced lambda
/// bodies or the base array's elements). A chain of any `map`/`filter` mix under
/// an optional outermost `fold`/`reduce`, over the same proven base, fuses to one
/// loop; the recursion still reaches HOFs nested inside a spliced lambda body or a
/// declined chain's inner run (including a fold whose composition was declined,
/// whose map/filter prefix then fuses on its own).
fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbol_names: &HashMap<u32, String>,
    ops: &Ops,
    bases: &FxHashMap<Binding, &'static str>,
    templates: &FxHashMap<Binding, FnTemplate>,
) {
    if let Some(plan) = validate_chain(hir, arena, symbol_names, bases, templates) {
        let sig = hir.signal;
        let span = hir.span.clone();
        let owned = std::mem::replace(hir, Hir::error(span.clone()));
        let (terminal, stages, base) = take_chain(owned, plan, arena, templates);
        *hir = build_loop(terminal, stages, base, arena, ops, sig, span);
    }
    hir.for_each_child_mut(|c| rewrite(c, arena, symbol_names, ops, bases, templates));
}

/// The higher-order collection op a fused chain is built from, and the kind of
/// each *stage* in the unified pipeline (`Build::element`). Both take
/// `(lambda, coll)` and share the `(get`/`push`/`freeze)` index-walk over `coll`'s
/// array arm; they differ only in how a stage handles the threaded element value:
/// a `Map` stage transforms it and threads the result on, a `Filter` stage guards
/// the rest of the pipeline behind its predicate (an `if`).
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
enum Terminal {
    Collect { unfrozen: bool },
    Fold(Box<FoldTerminal>),
}

/// The moved-out parts of a `fold`/`reduce` terminal (the boxed `Terminal::Fold`
/// payload): the seed `init` and the combinator's two params + body.
struct FoldTerminal {
    init: Hir,
    acc_param: Binding,
    elem_param: Binding,
    body: Hir,
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

/// A recognized `(fold <lambda> <init> <coll>)` / `(reduce …)` call: the 2-param
/// combinator lambda, the seed `init`, and the collection (all borrowed). `None`
/// when `hir` is not a call to the canonical stdlib `fold`/`reduce` with exactly
/// three non-spliced arguments. `reduce` is `(def reduce fold)` — the same
/// left-fold, recognized by either name. A user redefinition shadows the name with
/// a non-primitive binding and is excluded (the `is_primitive` gate, as for
/// `map`/`filter`). `fold` is the only op that may be the chain's outermost
/// terminal — its scalar result is not a collection, so nothing chains over it.
fn fusable_fold_parts<'a>(
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
/// These bounds keep the splice a straight `(let [param elem] body)` per parameter
/// with no substitution or cell reasoning.
fn qualifies_lambda<'a>(
    lam: &'a Hir,
    arena: &BindingArena,
    arity: usize,
) -> Option<(&'a [Binding], &'a Hir)> {
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
    if rest_param.is_some() || params.len() != arity || !captures.is_empty() {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || body_disqualifies(body) {
        return None;
    }
    Some((params, body))
}

/// The body signal of a HOF's function argument at the given arity, or `None` if
/// it does not qualify. The argument is one of two forms:
///
/// - a **lambda literal** (`qualifies_lambda`), or
/// - a **`Var`** naming a same-unit template (`templates`) whose parameter count
///   matches `arity` — the named-function inlining path.
///
/// Returns the body's top signal, which the caller feeds to the reorder gate (the
/// two forms are gated identically). The template body qualified at collection, so
/// only the arity is re-checked here.
fn fn_arg_body_signal(
    lam: &Hir,
    arena: &BindingArena,
    templates: &FxHashMap<Binding, FnTemplate>,
    arity: usize,
) -> Option<Signal> {
    match &lam.kind {
        HirKind::Lambda { .. } => qualifies_lambda(lam, arena, arity).map(|(_, body)| body.signal),
        HirKind::Var(b) => {
            let t = templates.get(b)?;
            (t.params.len() == arity).then_some(t.body.signal)
        }
        _ => None,
    }
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

/// A validated fusable chain, ready for `take_chain`: whether the outermost op is
/// a `fold`/`reduce` terminal (a scalar accumulator), the inner `map`/`filter`
/// pipeline kinds in the order the walk encounters them (OUTER→INNER), and whether
/// the base is a mutable `@array` (so a Collect terminal emits the accumulator
/// unfrozen — the mutable-array arm).
struct ChainPlan {
    fold: bool,
    kinds: Vec<Hof>,
    mutable_base: bool,
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
fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    bases: &FxHashMap<Binding, &'static str>,
    templates: &FxHashMap<Binding, FnTemplate>,
) -> Option<ChainPlan> {
    let mut all_silent = true;
    let mut ops = 0usize;
    let mut cur = hir;

    // The optional outermost fold/reduce terminal (2-param combinator).
    let fold = if let Some((lam, _init, coll)) = fusable_fold_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fn_arg_body_signal(lam, arena, templates, 2)?);
        ops += 1;
        cur = coll;
        true
    } else {
        false
    };

    // The inner map/filter pipeline (1-param functions).
    let mut kinds = Vec::new();
    while let Some((hof, lam, coll)) = fusable_hof_parts(cur, arena, symbol_names) {
        all_silent &= reorder_safe(fn_arg_body_signal(lam, arena, templates, 1)?);
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
fn reorder_safe(sig: Signal) -> bool {
    sig.bits.subtract(SIG_ERROR).is_empty() && sig.propagates == 0
}

/// The proven array-ness of a fused chain's base — the fact that selects the
/// terminal's result arm (frozen vs unfrozen), mirroring the stdlib op's own
/// `(if (mutable? coll) acc (freeze acc))`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BaseKind {
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
fn classify_base(
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

/// Resolve a HOF's function argument to owned `(params, body)`, ready to splice:
///
/// - a **lambda literal** is *moved* out of the call (its parameters and body are
///   uniquely owned by the splice thereafter);
/// - a **`Var`** naming a same-unit template is *cloned* with fresh bindings and
///   HirIds (`clone_template`) — the definition persists, so nothing is moved.
///
/// Validation (`fn_arg_body_signal`) proved one of these holds at the required
/// arity, so the match is total.
fn take_fn_parts(
    lam: Hir,
    arena: &mut BindingArena,
    templates: &FxHashMap<Binding, FnTemplate>,
) -> (Vec<Binding>, Hir) {
    match lam.kind {
        HirKind::Lambda { params, body, .. } => (params, *body),
        HirKind::Var(b) => {
            let t = templates.get(&b).expect("validate_chain proved a template");
            clone_template(t, arena)
        }
        _ => unreachable!("validate_chain proved a lambda or a template Var"),
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
fn take_chain(
    mut expr: Hir,
    plan: ChainPlan,
    arena: &mut BindingArena,
    templates: &FxHashMap<Binding, FnTemplate>,
) -> (Terminal, Vec<(Hof, Binding, Hir)>, Hir) {
    let terminal = if plan.fold {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a fold call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("fold has 3 args").expr;
        let init = it.next().expect("fold has 3 args").expr;
        let coll = it.next().expect("fold has 3 args").expr;
        let (params, body) = take_fn_parts(lam, arena, templates);
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
        let (params, body) = take_fn_parts(lam, arena, templates);
        stages.push((hof, params[0], body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    stages.reverse();
    (terminal, stages, expr)
}

/// Node factory for the synthesized loop. Bundles the span and signal every
/// synthesized node carries and the arena for minting locals, so the fixed
/// `(get`/`push`/`freeze)` scaffold and the per-element transform/guard pipeline
/// build nodes uniformly. The synthesized helper calls and `if`/`let` scaffolding
/// carry `sig` — the original call's signal, a sound upper bound over every op in
/// the stdlib op's body — while spliced lambda bodies keep their own signals (they
/// are moved in whole). Bottom-up re-propagation (`hir/narrow.rs`) then rebuilds
/// the fused form's signal from these leaves without under-reporting.
struct Build<'a> {
    arena: &'a mut BindingArena,
    ops: &'a Ops,
    span: crate::syntax::Span,
    sig: Signal,
}

impl Build<'_> {
    fn node(&self, kind: HirKind) -> Hir {
        Hir::new(kind, self.span.clone(), self.sig)
    }
    fn var(&self, b: Binding) -> Hir {
        Hir::new(HirKind::Var(b), self.span.clone(), Signal::silent())
    }
    fn int(&self, n: i64) -> Hir {
        Hir::new(HirKind::Int(n), self.span.clone(), Signal::silent())
    }
    fn nil(&self) -> Hir {
        Hir::new(HirKind::Nil, self.span.clone(), Signal::silent())
    }
    fn call(&self, f: Binding, args: Vec<Hir>) -> Hir {
        self.node(HirKind::Call {
            func: Box::new(self.var(f)),
            args: args
                .into_iter()
                .map(|expr| CallArg {
                    expr,
                    spliced: false,
                })
                .collect(),
            is_tail: false,
        })
    }
    fn let_(&self, binding: Binding, value: Hir, body: Hir) -> Hir {
        self.node(HirKind::Let {
            bindings: vec![(binding, value)],
            body: Box::new(body),
        })
    }
    /// A fresh immutable local (accumulator, length, bound element, …).
    fn local(&mut self) -> Binding {
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_immutable = true;
        b
    }
    /// Retype a consumed lambda parameter to a plain immutable local: the lambda is
    /// gone, so the lowerer must give the parameter a local slot, not an argument
    /// slot.
    fn localize_param(&mut self, param: Binding) {
        let pi = self.arena.get_mut(param);
        pi.scope = BindingScope::Local;
        pi.is_immutable = true;
    }

    /// Build the per-element statement for a transform/guard pipeline over the
    /// current value `cur`, threading it through the remaining `stages` (in
    /// application order — innermost op first):
    ///
    /// - a **`Map`** stage transforms the value (`(let [param cur] body)`) and
    ///   threads the result on to the rest of the pipeline;
    /// - a **`Filter`** stage binds the current value once (`item`, since a guard
    ///   references it twice — the test and the pass-through) and continues the
    ///   pipeline only when its predicate passes, else `nil`;
    /// - the base case (no stages left) hands the surviving value to the
    ///   **terminal** (`Build::terminal`): a `push` (Collect) or a fold step (Fold).
    ///
    /// This one recursion realizes `map`, `filter`, `fold`, and any mix in a SINGLE
    /// loop: a `map`-only chain is all `Map` stages (the transforms nest, no `if`),
    /// a `filter`-only chain is all `Filter` stages (the element binds once, guards
    /// nest), a mixed chain interleaves the two, and a fold reuses the same stages
    /// with a scalar terminal — the intermediate array between any two adjacent
    /// stages (or between the pipeline and the fold) never exists.
    fn element(
        &mut self,
        stages: &mut std::vec::IntoIter<(Hof, Binding, Hir)>,
        fold: &mut Option<(Binding, Binding, Hir)>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match stages.next() {
            None => self.terminal(fold, acc, cur),
            Some((Hof::Map, param, body)) => {
                self.localize_param(param);
                let next = self.let_(param, cur, body);
                self.element(stages, fold, acc, next)
            }
            Some((Hof::Filter, param, pred)) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), pred);
                let then = self.element(stages, fold, acc, self.var(item));
                let guarded = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then),
                    else_branch: Box::new(self.nil()),
                });
                self.let_(item, cur, guarded)
            }
        }
    }

    /// The pipeline's innermost base case — how a surviving element value `cur`
    /// enters the accumulator. Built exactly once (the base of the single element
    /// statement), so the fold combinator (`Some`) is consumed here by `take`:
    ///
    /// - **Collect** (`None`): push `cur` into the `@array` accumulator.
    /// - **Fold** (`Some((acc_param, elem_param, body))`): one left-fold step —
    ///   rebind the combinator's two params (the current `acc`, and `cur`) and
    ///   reassign the scalar accumulator to the body's result:
    ///   `(assign acc (let [acc_param acc] (let [elem_param cur] body)))`.
    fn terminal(
        &mut self,
        fold: &mut Option<(Binding, Binding, Hir)>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match fold.take() {
            None => self.call(self.ops.push, vec![self.var(acc), cur]),
            Some((acc_param, elem_param, body)) => {
                self.localize_param(acc_param);
                self.localize_param(elem_param);
                let inner = self.let_(elem_param, cur, body);
                let step = self.let_(acc_param, self.var(acc), inner);
                self.node(HirKind::Assign {
                    target: acc,
                    value: Box::new(step),
                })
            }
        }
    }
}

/// Build the fused index-walk loop from the terminal, pipeline stages, and base
/// collection. The `(get` + index-walk) scaffold is fixed; the per-element body is
/// the unified transform/guard pipeline (`Build::element`) bottoming out at the
/// terminal, so `map`, `filter`, `fold`, and any mix all collapse to one loop with
/// one accumulator. The terminal picks the accumulator's shape and result:
///
/// ```text
/// Collect (map/filter):            Fold (fold/reduce):
/// (let [coll BASE]                 (let [seed INIT]
///   (let [len (length coll)]         (let [coll BASE]
///     (let [acc (@array)]              (let [len (length coll)]
///       (define i 0)                     (define acc seed)
///       (while (< i len)                 (define i 0)
///         <pipeline; push acc>           (while (< i len)
///         (assign i (+ i 1)))              <pipeline; assign acc (f acc _)>
///       (freeze acc))))                    (assign i (+ i 1)))
///                                        acc)))
/// ```
///
/// For a fold, `init` is bound to an immutable `seed` OUTERMOST so it evaluates
/// before the base collection — the source order of `(fold f init coll)` — even
/// though the loop needs `coll`/`len` first. The accumulator is a reassigned
/// scalar (mirrors the induction variable), never an `@array`.
///
/// The Collect terminal's `unfrozen` flag selects its result arm: an immutable
/// base freezes the accumulator; a mutable `@array` base returns it unfrozen (the
/// mutable-array arm — `validate_chain` proves a mutable base is a lone
/// `map`/`filter`, so a Fold terminal is never paired with it).
fn build_loop(
    terminal: Terminal,
    stages: Vec<(Hof, Binding, Hir)>,
    base: Hir,
    arena: &mut BindingArena,
    ops: &Ops,
    sig: Signal,
    span: crate::syntax::Span,
) -> Hir {
    let mut b = Build {
        arena,
        ops,
        span,
        sig,
    };
    let coll_b = b.local();
    let len_b = b.local();
    let i_b = b.arena.gensym();
    b.arena.get_mut(i_b).is_mutated = true; // the loop induction variable

    // Split the terminal into its seed (`init`, a fold only) and its per-element
    // base case (`fold` — `None` for Collect). The accumulator differs by terminal:
    // Collect fills a fresh `@array` (immutable binding, mutated in place); Fold
    // threads a reassigned scalar.
    let (init, mut fold, acc_b, unfrozen) = match terminal {
        Terminal::Collect { unfrozen } => (None, None, b.local(), unfrozen),
        Terminal::Fold(f) => {
            let FoldTerminal {
                init,
                acc_param,
                elem_param,
                body,
            } = *f;
            let acc = b.arena.gensym();
            b.arena.get_mut(acc).is_mutated = true;
            (Some(init), Some((acc_param, elem_param, body)), acc, false)
        }
    };

    // The per-element statement: thread (get coll i) through the pipeline.
    let elem0 = b.call(ops.get, vec![b.var(coll_b), b.var(i_b)]);
    let body_stmt = b.element(&mut stages.into_iter(), &mut fold, acc_b, elem0);

    let incr = b.node(HirKind::Assign {
        target: i_b,
        value: Box::new(b.call(ops.add, vec![b.var(i_b), b.int(1)])),
    });
    let while_loop = b.node(HirKind::While {
        cond: Box::new(b.call(ops.lt, vec![b.var(i_b), b.var(len_b)])),
        body: Box::new(b.node(HirKind::Begin(vec![body_stmt, incr]))),
    });
    let define_i = b.node(HirKind::Define {
        binding: i_b,
        value: Box::new(b.int(0)),
    });

    match init {
        // Collect — a fresh `@array` accumulator. An immutable base freezes it to
        // the result; a mutable `@array` base returns it unfrozen (type-preserving,
        // mirroring the stdlib arm `(if (mutable? coll) acc (freeze acc))`).
        None => {
            let result = if unfrozen {
                b.var(acc_b)
            } else {
                b.call(ops.freeze, vec![b.var(acc_b)])
            };
            let acc_body = b.node(HirKind::Begin(vec![define_i, while_loop, result]));
            let acc_let = b.let_(acc_b, b.call(ops.at_array, vec![]), acc_body);
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), acc_let);
            b.let_(coll_b, base, len_let)
        }
        // Fold — a scalar accumulator seeded by `init`, its final value the result.
        Some(init) => {
            let seed_b = b.local();
            let define_acc = b.node(HirKind::Define {
                binding: acc_b,
                value: Box::new(b.var(seed_b)),
            });
            let result = b.var(acc_b);
            let loop_body = b.node(HirKind::Begin(vec![
                define_acc, define_i, while_loop, result,
            ]));
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), loop_body);
            let coll_let = b.let_(coll_b, base, len_let);
            b.let_(seed_b, init, coll_let)
        }
    }
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

    /// Parity invariant the fusion recognition relies on: the canonical HOF
    /// exports defined in **core.lisp** (`fold`/`reduce`) resolve at a call site to
    /// an `is_primitive` binding, exactly as the **stdlib.lisp** exports
    /// (`map`/`filter`) do. Core exports are bound twice — as a full primitive by
    /// `bind_primitives` (from `meta`) and as the canonical override by
    /// `bind_compile_time_env` (the core-env, which wins name resolution and
    /// carries the correct value). The override must also be marked `is_primitive`
    /// (`analyze::bind_compile_time_env`, `is_primitive = true` for the core env),
    /// or a core HOF is invisible to every pass that keys on the flag — loop
    /// fusion here, dispatch monomorphization. A user redefinition still shadows
    /// with a non-primitive binding (the safety complement).
    ///
    /// The probes fold over an UNPROVEN collection (a function parameter, not a
    /// literal array) precisely so the `fold`/`reduce` call *survives* — a call
    /// over a proven immutable array now dissolves (that is exactly what this parity
    /// enables), leaving no callee to inspect. Declining on the unproven base keeps
    /// the call while still resolving its binding.
    #[test]
    fn core_lisp_hof_exports_are_primitive_like_stdlib() {
        // The binding a `(name …)` call resolves to (the winning shadow).
        fn callee_is_primitive(src: &str, name: &str) -> bool {
            let (hir, arena, names) = compile(src);
            fn find(
                h: &Hir,
                arena: &BindingArena,
                names: &HashMap<u32, String>,
                want: &str,
            ) -> Option<bool> {
                if let HirKind::Call { func, .. } = &h.kind {
                    if let Some(b) = super::unwrap_callee_binding(func) {
                        if names.get(&arena.get(b).name.0).map(String::as_str) == Some(want) {
                            return Some(arena.get(b).is_primitive);
                        }
                    }
                }
                let mut found = None;
                h.for_each_child(|c| found = found.or_else(|| find(c, arena, names, want)));
                found
            }
            find(&hir, &arena, &names, name).expect("call to the named op is present")
        }
        // core.lisp exports — primitive, exactly like the stdlib map/filter.
        assert!(
            callee_is_primitive("(defn ff [xs] (fold (fn [a x] (+ a x)) 0 xs))", "fold"),
            "core.lisp `fold` must resolve to a primitive binding (parity with map)",
        );
        assert!(
            callee_is_primitive("(defn rr [xs] (reduce (fn [a x] (+ a x)) 0 xs))", "reduce"),
            "core.lisp `reduce` must resolve to a primitive binding",
        );
        // Safety complement: a user redefinition shadows with a non-primitive one.
        assert!(
            !callee_is_primitive("(defn fold [f i c] i) (fold (fn [a x] a) 0 [1])", "fold"),
            "a user `fold` redefinition must NOT be primitive",
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

    /// Reorder gate on a MIXED chain: `(map f (filter p xs))` where the predicate
    /// is a variadic `>` (routes through `apply`, so NOT reorder-safe). A mixed
    /// chain is length ≥ 2, so it always carries the reorder requirement; the
    /// non-reorder-safe predicate declines the whole composition, and the chain
    /// falls back to fusing only its inner reorder-safe run — the `filter` fuses on
    /// the pre-order recursion and the outer `map` stays a plain call over the fused
    /// loop. (The fused loop lands beside `map`'s surviving lambda `f`; `lower_call`'s
    /// argument spill keeps that sound — `call-arg-across-loop.lisp`.) The
    /// reorder-safe mixed case fusing into ONE loop is pinned below.
    #[test]
    fn mixed_chain_with_non_reorder_safe_stage_fuses_inner_only() {
        let (hir, arena, names) =
            compile("(map (fn [x] (* x 2)) (filter (fn [w] (> w 1)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "the outer `map` must not fuse a non-reorder-safe composition; \
             callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == ">"),
            "the inner predicate must inline; callees were {cs:?}",
        );
    }

    /// A reorder-safe MIXED `(map f (filter p xs))` fuses to a SINGLE loop: both the
    /// `map` and `filter` dispatches are gone, both body ops (`*` and `even?`) run
    /// inline, there is exactly ONE accumulator (the intermediate survivor array
    /// between the `filter` and the `map` is gone), and one guard `if` (the filter
    /// stage). `even?` carries only `SIG_ERROR` and `*` is silent, so both are
    /// reorder-safe and the length-2 composition fuses. Fails before mixed fusion:
    /// the outer `map` survives as a plain call over the inner-fused filter.
    #[test]
    fn mixed_map_of_filter_reorder_safe_fuses_to_one_loop() {
        let (hir, arena, names) =
            compile("(map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map" || n == "filter"),
            "both HOF dispatches must be gone in a fused mixed chain; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "even?"),
            "both the transform and the predicate must inline; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "one loop, one accumulator — the intermediate survivor array is gone; \
             callees were {cs:?}",
        );
        // Two `if`s: the loop condition (every fused loop's `while`→`loop` lowering
        // emits one) plus exactly one filter guard — the single `filter` stage.
        assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
    }

    /// A reorder-safe MIXED `(filter q (map g xs))` fuses to a SINGLE loop with the
    /// map stage transforming first and the guard testing the transformed value: no
    /// `map`/`filter` dispatch, both ops (`*` and `even?`) inline, one accumulator
    /// (no intermediate mapped array), one guard `if`. Fails before mixed fusion:
    /// the outer `filter` survives as a plain call over the inner-fused map.
    #[test]
    fn mixed_filter_of_map_reorder_safe_fuses_to_one_loop() {
        let (hir, arena, names) =
            compile("(filter (fn [y] (even? y)) (map (fn [x] (* x 5)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map" || n == "filter"),
            "both HOF dispatches must be gone in a fused mixed chain; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "even?"),
            "both the transform and the predicate must inline; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "one loop, one accumulator — the intermediate mapped array is gone; \
             callees were {cs:?}",
        );
        // The loop condition `if` plus one filter guard — the single `filter` stage.
        assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
    }

    /// A three-stage mixed tower `(map h (filter p (map g xs)))` collapses to ONE
    /// loop: all three ops inline (`+`, `even?`, `*`), one accumulator (both
    /// intermediates gone), one guard `if`. Proves the pipeline nests to arbitrary
    /// depth across kinds, not just length 2.
    #[test]
    fn mixed_three_stage_tower_fuses_to_one_loop() {
        let (hir, arena, names) = compile(
            "(map (fn [z] (+ z 1)) \
               (filter (fn [y] (even? y)) \
                 (map (fn [x] (* x 3)) [1 2 3 4])))",
        );
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map" || n == "filter"),
            "every HOF dispatch must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "+")
                && cs.iter().any(|n| n == "even?")
                && cs.iter().any(|n| n == "*"),
            "all three stage bodies must inline; callees were {cs:?}",
        );
        assert_eq!(
            cs.iter().filter(|n| *n == "@array").count(),
            1,
            "one loop, one accumulator — both intermediate arrays are gone; \
             callees were {cs:?}",
        );
        // The loop condition `if` plus one filter guard — the tower has a single
        // `filter` stage among its three ops.
        assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
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

    /// The mutable-array arm (docs/impl/dissolution.md § "The mutable-array arm"):
    /// a single `map` over a proven **mutable** `@array` base fuses, but its result
    /// is left **unfrozen** — mirroring the stdlib arm `(if (mutable? coll) acc
    /// (freeze acc))`. The `map` dispatch and the closure are gone, the transform
    /// op inlines, and — the discriminator against the immutable arm — there is NO
    /// `freeze` call: the mutable accumulator IS the result. (The base `@[ … ]` and
    /// the accumulator are two `@array` calls; neither is frozen.)
    #[test]
    fn single_map_over_mutable_array_fuses_unfrozen() {
        let (hir, arena, names) = compile("(map (fn [x] (* x 2)) @[1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "a mutable `@array` base must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "*"),
            "the transform op must inline; callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "freeze"),
            "a mutable-array map returns the accumulator UNFROZEN; callees were {cs:?}",
        );
    }

    /// The mutable arm reaches a `Var`-bound `@array` too (the alias proof resolves
    /// the base to the `@array` keyword): `(let [xs @[ … ]] (map f xs))` fuses to
    /// the unfrozen index-walk loop, exactly as the call-site literal does.
    #[test]
    fn map_over_var_bound_mutable_array_fuses_unfrozen() {
        let (hir, arena, names) = compile("(let [xs @[1 2 3]] (map (fn [x] (* x 2)) xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "a Var-bound mutable `@array` base must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            !cs.iter().any(|n| n == "freeze"),
            "the mutable result is unfrozen; callees were {cs:?}",
        );
    }

    /// A single `filter` over a mutable `@array` fuses to the guarded-push loop
    /// with an **unfrozen** result (the surviving-element accumulator is itself
    /// mutable), mirroring the stdlib arm. The `filter` dispatch and closure are
    /// gone, the predicate inlines under an `if`, and no `freeze` runs.
    #[test]
    fn single_filter_over_mutable_array_fuses_unfrozen() {
        let (hir, arena, names) = compile("(filter (fn [x] (> x 2)) @[1 2 3 4])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "a mutable `@array` base must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
        assert!(
            !cs.iter().any(|n| n == "freeze"),
            "a mutable-array filter returns the accumulator UNFROZEN; callees were {cs:?}",
        );
    }

    /// Safety: a `fold` over a mutable `@array` base is NOT fused. `fold` first
    /// snapshots its input (`(->array coll)` copies a mutable array) and walks the
    /// copy; a fused fold would walk the LIVE base, so a mutating combinator would
    /// diverge from the stdlib fold. The `fold` call survives.
    #[test]
    fn fold_over_mutable_array_is_not_fused() {
        let (hir, arena, names) = compile("(fold (fn [a x] (+ a x)) 0 @[1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "fold"),
            "a fold over a mutable base must not fuse; callees were {cs:?}",
        );
        assert!(count_lambdas(&hir) >= 1, "the fold closure must survive");
    }

    /// Safety: a COMPOSITION over a mutable `@array` base does not fuse into one
    /// loop — the fused loop would interleave the ops against the LIVE base, where
    /// a later op's lambda mutating the base could change an earlier op's reads
    /// (the staged stdlib ops each run to completion over a fresh array first). The
    /// outer op declines; the pre-order recursion still fuses the innermost single
    /// `map` (sound in isolation — its result a fresh mutable array the outer op
    /// then walks), so exactly one `map` and one closure — the outer — survive, and
    /// the inner transform inlines.
    #[test]
    fn composition_over_mutable_array_fuses_inner_only() {
        let (hir, arena, names) = compile("(map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) @[1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert_eq!(
            count_callee(&hir, &arena, &names, "map"),
            1,
            "only the outer `map` survives a mutable-base composition; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 1, "only the outer closure survives");
        assert!(
            cs.iter().any(|n| n == "*"),
            "the inner transform still inlines on the recursion; callees were {cs:?}",
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

    /// Count the calls to a given op name in the tree.
    fn count_callee(
        h: &Hir,
        arena: &BindingArena,
        names: &HashMap<u32, String>,
        want: &str,
    ) -> usize {
        callees(h, arena, names)
            .iter()
            .filter(|n| *n == want)
            .count()
    }

    /// A single `(fold f init xs)` dissolves to a **scalar** accumulator loop: the
    /// `fold` dispatch is gone, no closure survives, the fold body op (`+`) runs
    /// inline, and — unlike `map`/`filter` — there is NO `@array` and NO `freeze`
    /// (the accumulator is a reassigned scalar, the result is its final value).
    /// Fails before fold fusion lands: the `fold` call and the `(fn [a x] …)`
    /// closure are both present.
    #[test]
    fn single_fold_dissolves_to_scalar_accumulator() {
        let (hir, arena, names) = compile("(fold (fn [a x] (+ a x)) 0 [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold"),
            "the `fold` dispatch must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "+"),
            "the fold body op `+` must run inline; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "a fold's accumulator is a scalar — no `@array`; callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "freeze"),
            "a scalar fold accumulator is never frozen; callees were {cs:?}",
        );
        // The lowered scalar loop has exactly one `if` — the loop condition.
        assert_eq!(count_ifs(&hir), 1, "only the loop-condition `if`, no guard");
    }

    /// `(fold f init (map g xs))` fuses to ONE scalar loop — the map-reduce shape:
    /// both `fold` and `map` dispatches gone, both body ops (`+` and `*`) inline,
    /// and NO array anywhere (the map stage transforms the value straight into the
    /// fold step, so the intermediate array the `map` would have built never
    /// exists). Fails before fold fusion: a `fold` and a `map` call, two closures.
    #[test]
    fn fold_of_map_fuses_to_one_scalar_loop() {
        let (hir, arena, names) =
            compile("(fold (fn [a x] (+ a x)) 0 (map (fn [x] (* x 2)) [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold" || n == "map"),
            "both the `fold` and `map` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "+") && cs.iter().any(|n| n == "*"),
            "both the fold step and the map transform must inline; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "map-into-fold mints NO array — the map's result feeds the fold step; \
             callees were {cs:?}",
        );
        assert!(!cs.iter().any(|n| n == "freeze"), "no array to freeze");
        assert_eq!(count_ifs(&hir), 1, "only the loop-condition `if`, no guard");
    }

    /// `(fold f init (filter p xs))` fuses to ONE scalar loop with a guarded fold
    /// step: both dispatches gone, both body ops (`+` and `even?`) inline, NO array
    /// (scalar accumulator), and two `if`s — the loop condition plus the single
    /// `filter` guard (only survivors reach the fold step). Fails before fold
    /// fusion: a `fold` and a `filter` call, two closures.
    #[test]
    fn fold_of_filter_fuses_to_one_scalar_loop() {
        let (hir, arena, names) =
            compile("(fold (fn [a x] (+ a x)) 0 (filter (fn [y] (even? y)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold" || n == "filter"),
            "both the `fold` and `filter` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert!(
            cs.iter().any(|n| n == "+") && cs.iter().any(|n| n == "even?"),
            "both the fold step and the predicate must inline; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "filter-into-fold mints NO array; callees were {cs:?}",
        );
        assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
    }

    /// `reduce` is `(def reduce fold)` — the same left-fold, recognized by its own
    /// name. `(reduce f init xs)` dissolves exactly as `fold` does.
    #[test]
    fn reduce_dissolves_like_fold() {
        let (hir, arena, names) = compile("(reduce (fn [a x] (+ a x)) 0 [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "reduce" || n == "fold"),
            "the `reduce` dispatch must be gone; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "reduce fuses to a scalar accumulator; callees were {cs:?}",
        );
    }

    /// The reorder gate counts the fold as an op: a lone fold (length 1) threads
    /// its accumulator strictly in element order — exactly the stdlib fold — so it
    /// never reorders and fuses even with a NON-reorder-safe body (`>` routes
    /// through `apply`). The single-op path carries no reorder requirement.
    #[test]
    fn single_fold_with_non_reorder_safe_body_still_fuses() {
        let (hir, arena, names) = compile("(fold (fn [a x] (if (> a x) a x)) 0 [3 1 2])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold"),
            "a lone fold has no reorder gate and must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    }

    /// A fold composition with a NON-reorder-safe prefix stage declines the whole
    /// composition (length ≥ 2 carries the reorder requirement) and falls back to
    /// fusing only the inner reorder-safe run: the inner `filter` fuses on the
    /// recursion, and the outer `fold` stays a plain call over the fused loop. (The
    /// fused loop lands beside the fold's surviving lambda argument; `lower_call`'s
    /// argument spill keeps that sound — `call-arg-across-loop.lisp`.)
    #[test]
    fn fold_over_non_reorder_safe_prefix_fuses_inner_only() {
        let (hir, arena, names) =
            compile("(fold (fn [a x] (+ a x)) 0 (filter (fn [w] (> w 1)) [1 2 3 4]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "fold"),
            "the outer `fold` must not fuse a non-reorder-safe composition; \
             callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "filter"),
            "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
        );
    }

    /// Safety: a user redefinition of `fold` shadows the core binding with a
    /// non-primitive one, so it is never rewritten. The user's `fold` call survives.
    #[test]
    fn user_shadowed_fold_is_not_fused() {
        let (hir, arena, names) = compile("(defn fold [f i c] i) (fold (fn [a x] a) 0 [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "fold"),
            "a user `fold` must not be rewritten; callees were {cs:?}",
        );
    }

    /// Safety: a capturing fold lambda is left alone (its body references a free
    /// variable, so splicing it at the call site is out of scope). The `fold` call
    /// survives.
    #[test]
    fn capturing_fold_lambda_is_not_fused() {
        let (hir, arena, names) = compile("(let [k 10] (fold (fn [a x] (+ a (+ x k))) 0 [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "fold"),
            "a capturing fold lambda must not fuse; callees were {cs:?}",
        );
        assert!(count_lambdas(&hir) >= 1, "the closure must survive");
    }

    /// A fold over a `Var`-bound immutable array fuses — the base-alias proof and
    /// the scalar terminal compose, exactly as they do for `map`/`filter`.
    #[test]
    fn fold_over_var_bound_immutable_array_fuses() {
        let (hir, arena, names) = compile("(let [xs [1 2 3 4]] (fold (fn [a x] (+ a x)) 0 xs))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold"),
            "a Var-bound base must fuse; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "scalar fold accumulator; callees were {cs:?}",
        );
    }

    /// Named same-unit function inlining (docs/impl/dissolution.md § "Named
    /// same-unit functions"): a `map` whose function argument is a `Var` naming a
    /// top-level `(defn dbl …)` fuses just as an inline lambda does — the `map`
    /// dispatch is gone and `dbl`'s body is CLONED inline. The definition PERSISTS
    /// (it is cloned, not moved), so its own `(fn …)` still stands — hence the body
    /// op `*` now appears TWICE (the surviving definition + the inlined copy) where
    /// before fusion it appeared once and the `map` call survived.
    #[test]
    fn named_map_fn_inlines() {
        let (hir, arena, names) = compile("(defn dbl [x] (* x 2)) (map dbl [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "the `map` dispatch must be gone when the fn is a named defn; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "*"),
            2,
            "`dbl`'s body op appears twice — the surviving definition plus the \
             inlined copy; callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == "freeze"),
            "the fused loop freezes one accumulator; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            1,
            "one fused accumulator; callees were {cs:?}",
        );
    }

    /// A named 2-parameter combinator inlines into a fused fold: `(defn mul …)` used
    /// as `(fold mul 1 xs)` dissolves to the scalar-accumulator loop with `mul`'s
    /// body spliced in (so `*` appears twice — definition + inlined copy), and the
    /// `fold` dispatch is gone. The body op is `*`, not `+`: the loop scaffold's own
    /// `(+ i 1)` increment uses `+`, so `+` would not be a clean discriminator.
    #[test]
    fn named_fold_fn_inlines() {
        let (hir, arena, names) = compile("(defn mul [a b] (* a b)) (fold mul 1 [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "fold"),
            "the `fold` dispatch must be gone for a named combinator; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "*"),
            2,
            "`mul`'s body op appears twice (definition + inlined); callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            0,
            "a fold accumulator is scalar — no `@array`; callees were {cs:?}",
        );
    }

    /// A composition of a named function with itself fuses to ONE loop: `(map dbl
    /// (map dbl xs))` collapses both dispatches and inlines two copies of `dbl`'s
    /// body (so `*` appears three times — the definition plus two inlined copies)
    /// over a single accumulator.
    #[test]
    fn named_fn_composition_fuses_to_one_loop() {
        let (hir, arena, names) = compile("(defn dbl [x] (* x 2)) (map dbl (map dbl [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "both `map` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "*"),
            3,
            "definition + two inlined copies of `dbl`; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            1,
            "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
        );
    }

    /// A named function whose body is a `let` inlines: the clone whitelist admits
    /// `let`, freshening the let's own binding (`y`) with a fresh id per call site
    /// exactly as it freshens the parameters. The `map` dispatch is gone, and the
    /// body ops (`*` and `+`) appear TWICE — the surviving definition plus the
    /// inlined copy. Fails before the let-body clone widening lands: the body
    /// declines and the `map` call survives.
    #[test]
    fn named_fn_with_let_body_inlines() {
        let (hir, arena, names) = compile("(defn g [x] (let [y (* x 2)] (+ y 1))) (map g [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "a named fn with a `let` body must inline; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "*"),
            2,
            "`g`'s `*` appears twice — the surviving definition plus the inlined \
             copy; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            1,
            "one fused accumulator; callees were {cs:?}",
        );
    }

    /// Decline: a named function whose body introduces a binding through a form the
    /// clone whitelist does NOT cover (a `match` pattern — `let` is admitted, but a
    /// `match` binding is not) stays a plain `map` call, so the definition's own
    /// pattern bindings are never duplicated. The whitelist is a positive list of
    /// pure-expression forms plus `let`; anything else declines
    /// correct-by-construction.
    #[test]
    fn named_fn_with_match_body_declines() {
        let (hir, arena, names) = compile("(defn g [x] (match x _ (* x 2))) (map g [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a named fn with a `match` body must not inline; callees were {cs:?}",
        );
    }

    /// A `let`-body named function CLONED at two call sites in one composition:
    /// `(map g (map g xs))` where `g` has a `let` body inlines both copies into one
    /// loop. The let's own binding is re-minted with a fresh id per copy, so the two
    /// spliced bodies never collide in the region walk's per-id side tables — the
    /// hazard the alpha-renaming clone exists to prevent. `g`'s body op `*` appears
    /// THREE times (the definition plus two inlined copies) over one accumulator.
    /// (`*` is the clean discriminator, not `+`: the loop scaffold's own `(+ i 1)`
    /// increment also uses `+`.)
    #[test]
    fn named_let_body_fn_composition_fuses_to_one_loop() {
        let (hir, arena, names) =
            compile("(defn g [x] (let [y (* x 2)] (+ y 1))) (map g (map g [1 2 3]))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "both `map` dispatches must be gone; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "*"),
            3,
            "definition + two inlined copies of `g`'s `*`; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            1,
            "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
        );
    }

    /// Safety: a `let`-bound local function that CAPTURES a free variable is not
    /// inlined (captures are non-empty, so the body would reference an out-of-scope
    /// local once spliced). The `map` call survives.
    #[test]
    fn named_capturing_local_fn_declines() {
        let (hir, arena, names) =
            compile("(let [k 10] (let [g (fn [x] (+ x k))] (map g [1 2 3])))");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a capturing local fn must not inline; callees were {cs:?}",
        );
    }

    /// Safety: a `Var` argument whose binding is NOT a lambda (here an integer) is
    /// left alone — there is no function template to inline, so fusion declines and
    /// the `map` call survives.
    #[test]
    fn named_non_lambda_var_declines() {
        let (hir, arena, names) = compile("(def h 5) (map h [1 2 3])");
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter().any(|n| n == "map"),
            "a non-lambda Var arg must not inline; callees were {cs:?}",
        );
    }

    /// The definition survives inlining intact, so it is still usable as a
    /// first-class value: `(map dbl xs)` fuses AND `dbl` remains callable/referable
    /// elsewhere. The `map` is gone (fused), yet `dbl`'s lambda still stands (the
    /// inline cloned it rather than consuming it).
    #[test]
    fn named_fn_inlined_and_still_first_class() {
        let (hir, arena, names) =
            compile("(defn dbl [x] (* x 2)) (def ys (map dbl [1 2 3])) (dbl 9)");
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "map"),
            "the `map` must fuse; callees were {cs:?}",
        );
        assert!(
            count_lambdas(&hir) >= 1,
            "the cloned-from definition `dbl` must still stand as a lambda",
        );
    }
}
