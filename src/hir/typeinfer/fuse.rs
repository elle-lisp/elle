//! Map-chain loop fusion — the first closure dissolution (docs/impl/dissolution.md).
//!
//! At a call `(map f xs)` where `xs` is a statically-proven immutable array and
//! `f` is a non-capturing single-parameter lambda written at the call site, this
//! rewrites the cross-unit `map` dispatch to the index-walk loop `map`'s own
//! array arm runs (`src/stdlib.lisp`) — but with `f`'s body **spliced inline**
//! rather than called through a closure value. The closure ceases to exist: no
//! per-element closure allocation, no indirect call. A composition
//! `(map g (map f xs))` fuses to a **single** loop whose element expression
//! nests the transforms, so the intermediate array the inner `map` would have
//! allocated never exists.
//!
//! ## Why this shape, here
//!
//! The pass emits *surface* HIR — plain `while`/`push`/`freeze`, the same shape
//! `map`'s body has before functionalization — and runs in `regularize`
//! (`src/hir/regularize.rs`) **before** `functionalize`. So every downstream
//! pass consumes the fused loop exactly as it consumes `map`'s own body: the
//! `while` becomes a `loop`/`recur`, `push` monomorphizes to `%push-array-mut`
//! on the proven `@array` accumulator, region inference frees the accumulator by
//! subtree drop. The pass never hand-builds a `loop`/`recur` or a capture cell.
//!
//! It mirrors the container-dispatch monomorphization (`monomorphize.rs`):
//! recognize a proven-type call across the compile-unit boundary (the callee is
//! `is_primitive` — a `bind_primitives` stdlib export — and named `map`; a user
//! redefinition shadows it with a non-primitive binding and is left alone) and
//! collapse it to the direct form the proof selects.
//!
//! ## Legality
//!
//! Fusion preserves the program's value. A single `map` also preserves the exact
//! per-element evaluation order (the loop applies `f` left to right, identically
//! to `map`), so it needs no purity gate. A **composition** interleaves the
//! transforms (`f x0; g …; f x1; g …`) rather than running all of `f` then all
//! of `g` — a reorder observable only through sequencing effects — so each
//! lambda body in a chain of length ≥ 2 must be free of them (`reorder_safe`):
//! no yield/I/O/emit/FFI/halt (a non-capturing lambda's only cross-element
//! channel is such an effect). `SIG_ERROR` is permitted; see `reorder_safe`.

use super::unwrap_callee_binding;
use crate::hir::arena::{BindingArena, BindingScope};
use crate::hir::binding::Binding;
use crate::hir::expr::{CallArg, Hir, HirKind};
use crate::primitives::def::RetType;
use crate::signals::{Signal, SIG_ERROR};
use crate::symbol::SymbolTable;
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
    rewrite(hir, arena, &symbol_names, &ops);
}

/// Pre-order walk: try to fuse a `map` chain rooted at `hir` (consuming the whole
/// chain, including its inner `map` calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested `map`s in the spliced lambda
/// bodies or the base array's elements).
fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbol_names: &HashMap<u32, String>,
    ops: &Ops,
) {
    if let Some(len) = validate_chain(hir, arena, symbol_names) {
        let sig = hir.signal;
        let span = hir.span.clone();
        let owned = std::mem::replace(hir, Hir::error(span.clone()));
        let (transforms, base) = take_chain(owned, len);
        *hir = build_loop(transforms, base, arena, ops, sig, span);
    }
    hir.for_each_child_mut(|c| rewrite(c, arena, symbol_names, ops));
}

/// A recognized `(map <lambda> …)` call: the lambda argument and the collection
/// argument, both borrowed. `None` when `hir` is not a call to the canonical
/// stdlib `map` with exactly two non-spliced arguments.
fn fusable_map_parts<'a>(
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
    if !bi.is_primitive || symbol_names.get(&bi.name.0).map(String::as_str) != Some("map") {
        return None;
    }
    Some((&args[0].expr, &args[1].expr))
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

/// Validate that `hir` is a fusable `map` chain and return its length (the number
/// of nested `map`s). The chain bottoms out at a proven immutable array; every
/// lambda qualifies (`qualifies_lambda`); and for a composition (length ≥ 2)
/// every lambda body is `reorder_safe` (the reordering gate — see the module doc).
fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Option<usize> {
    let mut len = 0usize;
    let mut all_silent = true;
    let mut cur = hir;
    while let Some((lam, coll)) = fusable_map_parts(cur, arena, symbol_names) {
        let (_, body) = qualifies_lambda(lam, arena)?;
        all_silent &= reorder_safe(body.signal);
        len += 1;
        cur = coll;
    }
    if len == 0 || !coll_is_immutable_array(cur, arena, symbol_names) {
        return None;
    }
    if len >= 2 && !all_silent {
        return None;
    }
    Some(len)
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

/// Is `expr` a statically-proven immutable array — an array literal (a call to
/// the `array` primitive, `RetType::Array`) or an immutable-`Var` alias of one?
/// The same proof dead-arm pruning reads at this stage (`typeinfer/prune.rs`).
fn coll_is_immutable_array(
    expr: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> bool {
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

/// Consume a validated chain of `len` nested `map` calls, returning the per-
/// element transforms in **application order** (innermost `map`'s `f` first) and
/// the base collection expression. Validation (`validate_chain`) guarantees the
/// structure, so the destructuring is total.
fn take_chain(mut expr: Hir, len: usize) -> (Vec<(Binding, Hir)>, Hir) {
    let mut transforms = Vec::with_capacity(len);
    for _ in 0..len {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a map call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("map has 2 args").expr;
        let coll = it.next().expect("map has 2 args").expr;
        let HirKind::Lambda { params, body, .. } = lam.kind else {
            unreachable!("validate_chain proved a lambda");
        };
        transforms.push((params[0], *body));
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    transforms.reverse();
    (transforms, expr)
}

/// Build the fused index-walk loop from the transforms and base collection.
///
/// ```text
/// (let [coll BASE]
///   (let [len (length coll)]
///     (let [acc (@array)]
///       (define i 0)
///       (while (< i len)
///         (push acc (let [p0 (get coll i)] B0 … as nested lets …))
///         (assign i (+ i 1)))
///       (freeze acc))))
/// ```
///
/// The synthesized helper calls carry `sig` (the original `map` call's signal, a
/// sound upper bound over every op in `map`'s body); the spliced lambda bodies
/// keep their own signals. Bottom-up re-propagation (`hir/narrow.rs`) then
/// rebuilds the fused form's signal from these leaves without under-reporting.
fn build_loop(
    transforms: Vec<(Binding, Hir)>,
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

    // The per-element expression: (get coll i) fed through each transform, each
    // a (let [param elem] body). The parameter — no longer a lambda parameter,
    // since the lambda is consumed — is retyped to a plain immutable local so
    // the lowerer allocates it a local slot, not an argument slot.
    let mut elem = call(ops.get, vec![var(coll_b), var(i_b)]);
    for (param, body) in transforms {
        let pi = arena.get_mut(param);
        pi.scope = BindingScope::Local;
        pi.is_immutable = true;
        elem = node(HirKind::Let {
            bindings: vec![(param, elem)],
            body: Box::new(body),
        });
    }

    let push_stmt = call(ops.push, vec![var(acc_b), elem]);
    let incr = node(HirKind::Assign {
        target: i_b,
        value: Box::new(call(ops.add, vec![var(i_b), int(1)])),
    });
    let while_loop = node(HirKind::While {
        cond: Box::new(call(ops.lt, vec![var(i_b), var(len_b)])),
        body: Box::new(node(HirKind::Begin(vec![push_stmt, incr]))),
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
