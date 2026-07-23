//! HOF-chain loop fusion — the first closure dissolution (docs/impl/dissolution.md).
//!
//! At a call `(map f xs)` / `(filter p xs)` where `xs` is a statically-proven
//! immutable array and the lambda is a non-capturing single-parameter one written
//! at the call site, this rewrites the cross-unit stdlib dispatch to the
//! index-walk loop that op's own array arm runs (`src/stdlib.lisp`) — but with the
//! lambda body **spliced inline** rather than called through a closure value. The
//! closure ceases to exist: no per-element closure allocation, no indirect call.
//! `map` pushes each transform's result; `filter` pushes the element itself under
//! an `if` guard. A composition — `(map g (map f xs))`, `(filter q (filter p xs))`,
//! or any mix like `(map f (filter p xs))` — fuses to a **single** loop through one
//! unified transform/guard pipeline (`build_loop`/`Build::element`): each op is a
//! *stage* (a `map` transforms the threaded value; a `filter` guards it), the
//! stages nest in application order, and the intermediate array the inner op would
//! have allocated never exists. `map`-only and `filter`-only chains are just the
//! all-transform and all-guard ends of that one pipeline.
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
    rewrite(hir, arena, &symbol_names, &ops, &bases);
}

/// Pre-order walk: try to fuse a HOF chain rooted at `hir` (consuming the whole
/// chain, including its inner HOF calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested HOFs in the spliced lambda
/// bodies or the base array's elements). A chain of any `map`/`filter` mix over
/// the same proven base fuses to one loop; the recursion still reaches HOFs nested
/// inside a spliced lambda body or a declined chain's inner run.
fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbol_names: &HashMap<u32, String>,
    ops: &Ops,
    bases: &FxHashMap<Binding, &'static str>,
) {
    if let Some(kinds) = validate_chain(hir, arena, symbol_names, bases) {
        let sig = hir.signal;
        let span = hir.span.clone();
        let owned = std::mem::replace(hir, Hir::error(span.clone()));
        let (stages, base) = take_chain(owned, kinds);
        *hir = build_loop(stages, base, arena, ops, sig, span);
    }
    hir.for_each_child_mut(|c| rewrite(c, arena, symbol_names, ops, bases));
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

/// Validate that `hir` is a fusable HOF chain and return its per-op kinds in the
/// order the walk encounters them (OUTER→INNER). The chain bottoms out at a proven
/// immutable array; every op is `map` or `filter` (in any mix); every lambda
/// qualifies (`qualifies_lambda`); and for a composition (length ≥ 2 — homogeneous
/// or mixed) every lambda body is `reorder_safe` (the reordering gate — see the
/// module doc). A mixed chain is always length ≥ 2, so it always carries the
/// reorder requirement; a non-reorder-safe stage declines the whole composition,
/// and the pre-order recursion (`rewrite`) still fuses its inner reorder-safe run.
///
/// The walk stops at the first node that is not a fusable `map`/`filter` call; that
/// node is the base candidate. If it is not a proven immutable array (e.g. the
/// chain never reaches one, or a lambda failed `qualifies_lambda`), fusion declines
/// and the recursion retries at the inner calls.
fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    bases: &FxHashMap<Binding, &'static str>,
) -> Option<Vec<Hof>> {
    let mut kinds = Vec::new();
    let mut all_silent = true;
    let mut cur = hir;
    while let Some((hof, lam, coll)) = fusable_hof_parts(cur, arena, symbol_names) {
        let (_, body) = qualifies_lambda(lam, arena)?;
        all_silent &= reorder_safe(body.signal);
        kinds.push(hof);
        cur = coll;
    }
    if kinds.is_empty() || !coll_is_immutable_array(cur, arena, symbol_names, bases) {
        return None;
    }
    if kinds.len() >= 2 && !all_silent {
        return None;
    }
    Some(kinds)
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

/// Consume a validated chain, returning its per-element **stages**
/// (`(hof, param, body)`) in **application order** (innermost op first) and the
/// base collection expression. `kinds` is the chain's per-op kinds in outer→inner
/// order (from `validate_chain`), zipped with the lambdas extracted in that same
/// order. A `map` stage's body is a transform; a `filter` stage's is a predicate.
/// Validation guarantees the structure, so the destructuring is total.
fn take_chain(mut expr: Hir, kinds: Vec<Hof>) -> (Vec<(Hof, Binding, Hir)>, Hir) {
    let mut stages = Vec::with_capacity(kinds.len());
    for hof in kinds {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a HOF call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("HOF has 2 args").expr;
        let coll = it.next().expect("HOF has 2 args").expr;
        let HirKind::Lambda { params, body, .. } = lam.kind else {
            unreachable!("validate_chain proved a lambda");
        };
        stages.push((hof, params[0], *body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    stages.reverse();
    (stages, expr)
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
    /// - the base case (no stages left) pushes the surviving value into `acc`.
    ///
    /// This one recursion realizes `map`, `filter`, and any mix in a SINGLE loop:
    /// a `map`-only chain is all `Map` stages (the transforms nest, no `if`), a
    /// `filter`-only chain is all `Filter` stages (the element binds once, guards
    /// nest), and a mixed chain interleaves the two — the intermediate array
    /// between any two adjacent stages never exists.
    fn element(
        &mut self,
        stages: &mut std::vec::IntoIter<(Hof, Binding, Hir)>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match stages.next() {
            None => self.call(self.ops.push, vec![self.var(acc), cur]),
            Some((Hof::Map, param, body)) => {
                self.localize_param(param);
                let next = self.let_(param, cur, body);
                self.element(stages, acc, next)
            }
            Some((Hof::Filter, param, pred)) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), pred);
                let then = self.element(stages, acc, self.var(item));
                let guarded = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then),
                    else_branch: Box::new(self.nil()),
                });
                self.let_(item, cur, guarded)
            }
        }
    }
}

/// Build the fused index-walk loop from the pipeline stages and base collection.
/// The `(get`/`push`/`freeze)` scaffold is fixed; the per-element body is the
/// unified transform/guard pipeline (`Build::element`), so `map`, `filter`, and
/// any mix all collapse to one loop with one accumulator:
///
/// ```text
/// (let [coll BASE]
///   (let [len (length coll)]
///     (let [acc (@array)]
///       (define i 0)
///       (while (< i len)
///         <element pipeline over (get coll i)>
///         (assign i (+ i 1)))
///       (freeze acc))))
/// ```
fn build_loop(
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
    let acc_b = b.local();
    let i_b = b.arena.gensym();
    b.arena.get_mut(i_b).is_mutated = true; // the loop induction variable

    // The per-element statement: thread (get coll i) through the pipeline.
    let elem0 = b.call(ops.get, vec![b.var(coll_b), b.var(i_b)]);
    let body_stmt = b.element(&mut stages.into_iter(), acc_b, elem0);

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
    let freeze = b.call(ops.freeze, vec![b.var(acc_b)]);
    let acc_body = b.node(HirKind::Begin(vec![define_i, while_loop, freeze]));

    let acc_let = b.let_(acc_b, b.call(ops.at_array, vec![]), acc_body);
    let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), acc_let);
    b.let_(coll_b, base, len_let)
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
