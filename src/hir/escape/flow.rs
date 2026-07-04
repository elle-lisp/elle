//! The escape value-flow engine: the flow atoms, the interprocedural arg-return
//! summary, the tail/return seed collectors, and the backward-edge collection
//! that `analyze_escape` drives to a fixpoint. The analysis as a whole — its
//! facets, consumers, and precision characteristics — is documented in the parent
//! module (`super`, escape.rs).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};

/// An atom of value flow: the thing a (region-transparent) expression *is*.
/// Only these two carry escape-authority — a binding reference or a lambda
/// node. Everything else an expression can evaluate to is either an immediate
/// (no escape to track) or a freshly-minted region the solver names by an
/// allocation site (a `Call` result, an aggregate), which is not a binding or
/// lambda and so propagates no escape backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Atom {
    Binding(Binding),
    Lambda(HirId),
}

/// The context `tail_sources` needs to be interprocedural: the arena (to resolve
/// a callee `Var` and check it is an immutable, unmutated binding) and the
/// arg-return summary (which fixed-param indices each inlinable callee returns).
pub(super) struct TailCtx<'a> {
    pub(super) arena: &'a BindingArena,
    pub(super) arg_return: &'a FxHashMap<Binding, Vec<usize>>,
}

/// The arg-return summary: per inlinable lambda binding, which **fixed-param
/// indices** flow to its tail under interprocedural transparency. This is the
/// compile-time analogue of the solver's `try_inline_call` (`regions/walk.rs`),
/// which re-walks an inlinable callee body with params bound to the caller's arg
/// regions and returns the body's tail regions — for an arg-returning callee that
/// region is the arg's, so the arg escapes the caller's tail.
///
/// "Inlinable" mirrors the solver's `binding_lambda` exactly: a binding bound to a
/// `Lambda` by a `Let` or `Letrec` (never a top-level `Define` — the solver does
/// not populate `binding_lambda` there, so a `def`-bound callee is opaque and an
/// arg returned through it does NOT reach the caller's tail). The immutable/
/// unmutated guard is applied at the *consumption* site (`tail_sources`'s `Call`
/// arm), matching `try_inline_call`'s call-site check.
///
/// Computed to a fixpoint because the property is transitive: `(fn (z) (id z))`
/// returns its arg only once `id`'s own summary says `id` returns arg 0. The
/// fixpoint is monotonic (a returned index is never withdrawn), bounded by the
/// total param count, so it converges. The bounded re-walk in `try_inline_call`
/// stops at inline depth 4; this whole-program fixpoint can in principle propagate
/// through a deeper arg-return chain than the solver inlines, which would
/// *over-approximate* (mark a true escape the solver misses) — sound, and not
/// observed in the corpus; if a real-corpus golden ever surfaces it, bound the
/// propagation depth to match.
pub(super) fn compute_arg_return(
    hir: &Hir,
    arena: &BindingArena,
) -> FxHashMap<Binding, Vec<usize>> {
    // Recompute every inlinable lambda's returned-param set against the current
    // summary; visit Let/Letrec-bound lambdas anywhere in the tree, mirroring the
    // solver's `binding_lambda` population (never a top-level `Define`). Bodies are
    // processed inline during the walk, so nothing borrows past the visit.
    fn refine(ctx: &TailCtx, h: &Hir, out: &mut Vec<(Binding, Vec<usize>)>) {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &h.kind {
            for (b, init) in bindings {
                if let HirKind::Lambda { params, body, .. } = &init.kind {
                    let mut srcs = Vec::new();
                    tail_sources(ctx, body, &mut srcs);
                    let returned: Vec<usize> = params
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| srcs.contains(&Atom::Binding(**p)))
                        .map(|(i, _)| i)
                        .collect();
                    out.push((*b, returned));
                }
            }
        }
        h.for_each_child(|c| refine(ctx, c, out));
    }

    let mut summary: FxHashMap<Binding, Vec<usize>> = FxHashMap::default();
    loop {
        // Recompute against a frozen snapshot, then apply — so no lambda observes
        // a half-updated summary within one iteration. A Binding id reused across
        // sibling file-letrec re-defs appears more than once; source order means
        // the last application wins, matching the solver's `binding_lambda.insert`.
        let mut updates: Vec<(Binding, Vec<usize>)> = Vec::new();
        {
            let ctx = TailCtx {
                arena,
                arg_return: &summary,
            };
            refine(&ctx, hir, &mut updates);
        }
        let mut changed = false;
        for (b, returned) in updates {
            if summary.get(&b).map(Vec::as_slice) != Some(returned.as_slice()) {
                summary.insert(b, returned);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    summary
}

/// The atoms an expression region-transparently evaluates to — the set whose
/// regions the solver's `walk` returns for `hir`. Mirrors that walk's
/// result-flow exactly (walk.rs / walkrest.rs): a `Var`/`Lambda` is itself; a
/// binding/control form flows from its body/branches/tail; an `Eval`, allocating
/// intrinsic, loop, or immediate yields a fresh region or nothing, so no atom
/// flows out. A `Call` is opaque (fresh call-result region) *except* a tail call
/// to an arg-returning callee, which is region-transparent in the returned args
/// (the arg-return summary — see `compute_arg_return`).
fn tail_sources(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
    match &hir.kind {
        HirKind::Var(b) => out.push(Atom::Binding(*b)),
        HirKind::Lambda { .. } => out.push(Atom::Lambda(hir.id)),

        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => tail_sources(ctx, body, out),

        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            tail_sources(ctx, then_branch, out);
            tail_sources(ctx, else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                tail_sources(ctx, b, out);
            }
            if let Some(eb) = else_branch {
                tail_sources(ctx, eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                tail_sources(ctx, last, out);
            }
        }
        // And/Or short-circuit: any operand can be the result (the solver unions
        // all of them).
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                tail_sources(ctx, e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                tail_sources(ctx, body, out);
            }
        }

        // Region-transparent wrappers: the result is the wrapped value.
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => tail_sources(ctx, value, out),
        HirKind::DerefCell { cell } => tail_sources(ctx, cell, out),

        // `%get` borrows an element out of arg 0's region (region-transparent);
        // every other intrinsic allocates a fresh region or returns an immediate.
        HirKind::Intrinsic {
            op: IntrinsicOp::Get,
            args,
        } => {
            if let Some(a0) = args.first() {
                tail_sources(ctx, a0, out);
            }
        }

        // Interprocedural return: a tail call to an inlinable callee that returns
        // its fixed-param `i` is region-transparent in arg `i` — the call yields
        // whatever flowed into arg `i`. Mirrors the solver's `try_inline_call`
        // (the call-site immutable/unmutated guard included). A non-arg-returning
        // or opaque callee (no summary entry) descends into nothing, leaving the
        // call's fresh result region atomless, exactly as before.
        HirKind::Call { func, args, .. } => {
            if let HirKind::Var(b) = &func.kind {
                let bi = ctx.arena.get(*b);
                if bi.is_immutable && !bi.is_mutated {
                    if let Some(indices) = ctx.arg_return.get(b) {
                        for &i in indices {
                            if let Some(a) = args.get(i) {
                                tail_sources(ctx, &a.expr, out);
                            }
                        }
                    }
                }
            }
        }

        // Eval (fresh call-result region), While/Emit/Break/Recur (no value),
        // other intrinsics (fresh region), and immediates: no atom.
        _ => {}
    }
}

/// Atoms in **return position** — like `tail_sources`, but at an `assign` /
/// `set-cell!` that is itself a tail it ALSO seeds the *target* binding.
///
/// An `(assign b v)` expression evaluates to the value it just stored, so when it
/// is a function's tail the function returns that value — and `b` (the cell) now
/// holds a returned value. The solver folds the value's regions into
/// `binding_source_regions[b]`, so a reassigned mutable assigned a *fresh* result
/// (`(assign b (foo))`) at a tail holds a returned region even though the value
/// `(foo)` is atomless. Seeding the target here is the atom-level counterpart: it
/// lands `b` in the return facet, so the projection through
/// `binding_source_regions[b]` reconstructs that returned region.
///
/// Used ONLY for return-facet seeds. Edge collection keeps `tail_sources` (an
/// assign's *result* is its value, not the cell — `(let [x (assign c v)] …)` binds
/// `x` to `v`, never aliases the cell `c`).
pub(super) fn return_atoms(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
    match &hir.kind {
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => return_atoms(ctx, body, out),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            return_atoms(ctx, then_branch, out);
            return_atoms(ctx, else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                return_atoms(ctx, b, out);
            }
            if let Some(eb) = else_branch {
                return_atoms(ctx, eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                return_atoms(ctx, last, out);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                return_atoms(ctx, e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                return_atoms(ctx, body, out);
            }
        }
        HirKind::Return { value } | HirKind::MakeCell { value } => return_atoms(ctx, value, out),
        HirKind::DerefCell { cell } => return_atoms(ctx, cell, out),
        // The cell holds the returned value (seed it) AND the result is the stored
        // value (descend, so a nested assign / a returned binding/lambda value is
        // seeded too).
        HirKind::Assign { target, value } => {
            out.push(Atom::Binding(*target));
            return_atoms(ctx, value, out);
        }
        HirKind::SetCell { cell, value } => {
            if let HirKind::Var(b) = &cell.kind {
                out.push(Atom::Binding(*b));
            }
            return_atoms(ctx, value, out);
        }
        // Var / Lambda / `%get` / interprocedural Call / Define / Destructure /
        // fresh allocation / immediate: identical to the value-flow result.
        _ => tail_sources(ctx, hir, out),
    }
}

/// The **allocation-site `HirId`s** a value flows to a frontier (tail/return,
/// emit, send) through — the region-level half of the frontier facets, recorded
/// at the same positions `tail_sources` walks. Where `tail_sources` yields an
/// *atom* (a `Var`/`Lambda`), the consumer projects it through
/// `binding_source_regions` / `alloc_region`; this records the *atomless*
/// allocations the atom set cannot name — a `(%pair …)` / `(@array …)` / call
/// result / string literal reached in frontier position (`(yield (%pair 1 2))`,
/// a bare aggregate at a tail). The region solver projects these through its
/// `alloc_region` map: a `HirId` with no allocation (an arithmetic intrinsic, an
/// immediate) is simply absent there and contributes no region.
///
/// Deliberately an **over-approximation in the sound direction**: a `Lambda` and
/// every `Call` reached in frontier position are recorded (the latter even when
/// arg-returning, where the descended arg also carries the flow), because the
/// consumers want a SUPERSET of the truly-escaping regions — the ownership
/// Shared-seed must never *miss* an escape (a missed escape is a use-after-free,
/// `ownership/seeds.rs`), and the merge / branch-compensation gates read this as
/// an *exclusion* where over-excluding is leak-safe. A recorded `HirId` whose
/// region is a runtime placeholder (an immediate call result) is harmless: it is
/// already `not_ownable` / a non-candidate downstream, so projecting it changes
/// no decision. `%get` stays transparent (the borrowed element lives in arg 0's
/// region), exactly as in `tail_sources`.
pub(super) fn record_frontier_sites(ctx: &TailCtx, hir: &Hir, out: &mut FxHashSet<HirId>) {
    match &hir.kind {
        // A binding is an atom — projected via `binding_source_regions`, not here.
        HirKind::Var(_) => {}
        // A lambda's closure region is an allocation; a returned/emitted closure
        // crosses the frontier, so record it.
        HirKind::Lambda { .. } => {
            out.insert(hir.id);
        }

        // Region-transparent forms: the value is produced deeper — descend.
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => record_frontier_sites(ctx, body, out),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            record_frontier_sites(ctx, then_branch, out);
            record_frontier_sites(ctx, else_branch, out);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                record_frontier_sites(ctx, b, out);
            }
            if let Some(eb) = else_branch {
                record_frontier_sites(ctx, eb, out);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                record_frontier_sites(ctx, last, out);
            }
        }
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                record_frontier_sites(ctx, e, out);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                record_frontier_sites(ctx, body, out);
            }
        }
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => record_frontier_sites(ctx, value, out),
        HirKind::DerefCell { cell } => record_frontier_sites(ctx, cell, out),
        // `%get` borrows out of arg 0's region (transparent); every other
        // intrinsic either allocates (recorded — its `alloc_region` is the result)
        // or returns an immediate (absent from `alloc_region`, so harmless).
        HirKind::Intrinsic { op, args } => {
            if matches!(op, IntrinsicOp::Get) {
                if let Some(a0) = args.first() {
                    record_frontier_sites(ctx, a0, out);
                }
            } else {
                out.insert(hir.id);
            }
        }
        // A call: descend through an arg-returning callee (the returned arg carries
        // the flow, mirroring `tail_sources`), AND record the call's own result
        // region — an opaque call result crosses the frontier, and the over-mark on
        // an arg-returning/immediate result is harmless (see the fn doc).
        HirKind::Call { func, args, .. } => {
            if let HirKind::Var(b) = &func.kind {
                let bi = ctx.arena.get(*b);
                if bi.is_immutable && !bi.is_mutated {
                    if let Some(indices) = ctx.arg_return.get(b) {
                        for &i in indices {
                            if let Some(a) = args.get(i) {
                                record_frontier_sites(ctx, &a.expr, out);
                            }
                        }
                    }
                }
            }
            out.insert(hir.id);
        }
        // Fresh allocations with no atom: a string/quoted-compound literal or an
        // `Eval` result region.
        HirKind::String(_) | HirKind::QuoteConst(_) | HirKind::Eval { .. } => {
            out.insert(hir.id);
        }
        // Immediates and no-value forms (`While`/`Emit`/`Break`/`Recur`): no region.
        _ => {}
    }
}

/// The callee's declared `RegionEffect`, when the callee is an immutable,
/// unshadowed binding naming a declared primitive — mirrors the region solver's
/// `RegionInference::call_effect` (`regions/walk.rs`) so the store seeding matches
/// the solver's opaque-call edge classification. `None` for a user fn, a shadowed
/// name, or a non-`Var` callee.
fn call_effect(
    func: &Hir,
    arena: &BindingArena,
    call_class: &crate::hir::region::CallClassification,
) -> Option<crate::primitives::def::RegionEffect> {
    if let HirKind::Var(b) = &func.kind {
        let bi = arena.get(*b);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        call_class.effects.get(&bi.name).copied()
    } else {
        None
    }
}

/// Walk every node, recording everything the fixpoint needs: the value-flow
/// edges (each binding-introducing form maps its binding(s) to the atoms its
/// initializer flows from), each lambda's body-tail seeds (its own return),
/// store seeds (allocating intrinsics and native-declared stores), the `Emit`
/// fiber-boundary seeds, and each lambda's captured bindings (`lambda_captures`,
/// for the transitive capture consumer).
///
/// The capture facet has **no seed of its own**: a value escapes via capture only
/// by being pulled in transitively through `lambda_captures` once a genuine
/// frontier seed (return / store / fiber) marks its capturing closure escaping
/// (escape.md precision-point-3 — "the capture facet marks a value only when its
/// capturing closure escapes"). A closure captured but never returned/stored/sent
/// is called in place and escapes nothing, so the lexical-capture proxy
/// `is_captured` seeds escape nowhere here.
// A recursive accumulator-walk: each `&mut` sink collects one fixpoint input, kept
// as a distinct parameter rather than bundled so the arms read straightforwardly.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_flow(
    ctx: &TailCtx,
    hir: &Hir,
    call_class: &crate::hir::region::CallClassification,
    edges: &mut FxHashMap<Binding, Vec<Atom>>,
    return_seeds: &mut Vec<Atom>,
    fiber_seeds: &mut Vec<Atom>,
    other_seeds: &mut Vec<Atom>,
    return_sites: &mut FxHashSet<HirId>,
    fiber_sites: &mut FxHashSet<HirId>,
    lambda_captures: &mut FxHashMap<HirId, Vec<Binding>>,
) {
    let arena = ctx.arena;
    let add = |b: Binding, init: &Hir, edges: &mut FxHashMap<Binding, Vec<Atom>>| {
        let mut srcs = Vec::new();
        tail_sources(ctx, init, &mut srcs);
        if !srcs.is_empty() {
            edges.entry(b).or_default().extend(srcs);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                add(*b, init, edges);
            }
        }
        HirKind::Loop { bindings, .. } => {
            // Only the initial inits flow (the solver records `binding_regions`
            // from the init; `recur` re-binds but leaves `binding_regions`).
            for (b, init) in bindings {
                add(*b, init, edges);
            }
        }
        HirKind::Define { binding, value } => {
            add(*binding, value, edges);
        }
        HirKind::Assign {
            target: binding,
            value,
        } => {
            add(*binding, value, edges);
        }
        HirKind::SetCell { cell, value } => {
            if let HirKind::Var(b) = &cell.kind {
                add(*b, value, edges);
            }
        }
        HirKind::Destructure { pattern, value, .. } => {
            // A pattern binding aliases into the scrutinee's region(s), exactly
            // as the solver propagates `val_regions` to each bound var.
            for b in pattern.bindings().bindings {
                add(b, value, edges);
            }
        }
        HirKind::Match { value, arms } => {
            for (pat, _, _) in arms {
                for b in pat.bindings().bindings {
                    add(b, value, edges);
                }
            }
        }
        HirKind::Lambda { body, captures, .. } => {
            // The body's tail is the lambda's return (return facet) — both the
            // atoms (`return_atoms`) and the atomless allocation sites it reaches
            // (`record_frontier_sites`, the region-level half).
            return_atoms(ctx, body, return_seeds);
            record_frontier_sites(ctx, body, return_sites);
            // Record this lambda's upvalues for the transitive capture consumer:
            // if this lambda is later found escaping, every binding it captures
            // escapes with it.
            if !captures.is_empty() {
                lambda_captures.insert(hir.id, captures.iter().map(|c| c.binding).collect());
            }
        }
        // Store seeds — store-into-a-longer-lived-region escape. A value stored
        // into a freshly-allocated aggregate escapes its defining activation: the
        // region solver records the same as a `cross_region_refs` edge
        // `src=value-region → dst=aggregate-region` at these very intrinsics
        // (regions/walk/walkrest.rs's `Intrinsic` arm). Seed exactly the operands
        // that are those
        // edge SOURCES (the stored values) — never the aggregate (the store
        // target / edge destination, which does not escape by being written to):
        //   %pair       — every arg (car and cdr are both stored into the pair)
        //   %array-push — arg 1 (arg 0 is the collection / target)
        //   %put        — arg 2 (arg 0 is the collection, arg 1 the key)
        HirKind::Intrinsic { op, args } => match op {
            IntrinsicOp::Pair => {
                for a in args {
                    tail_sources(ctx, a, other_seeds);
                }
            }
            IntrinsicOp::Push | IntrinsicOp::PushArray | IntrinsicOp::PushArrayMut => {
                if let Some(v) = args.get(1) {
                    tail_sources(ctx, v, other_seeds);
                }
            }
            IntrinsicOp::Put
            | IntrinsicOp::PutStruct
            | IntrinsicOp::PutArray
            | IntrinsicOp::PutStructMut
            | IntrinsicOp::PutArrayMut => {
                if let Some(v) = args.get(2) {
                    tail_sources(ctx, v, other_seeds);
                }
            }
            _ => {}
        },
        // Fiber-boundary crossing — yield/emit. The emitted value is handed to the
        // resumer (a different activation, in general a different fiber), so it
        // escapes the emitting activation. There is no compile-time RC edge at an
        // `Emit` (the runtime incref in `handle_emit` keeps the operand alive past
        // the resume-site decref), so the fiber crossing is purely escape's to
        // record — the binding (`fiber_seeds`) and the atomless allocation site
        // (`fiber_sites`). The terminal-value boundary (a fiber body's return
        // crossing to the joiner) is already the return facet — a fiber body is a
        // lambda whose tail is seeded.
        HirKind::Emit { value, .. } => {
            // Fiber-facet seeds — kept separate from `other_seeds` (store/capture)
            // so the fiber-only binding set and the region-level fiber frontier can
            // be derived. Both the atoms and the atomless allocation sites.
            tail_sources(ctx, value, fiber_seeds);
            record_frontier_sites(ctx, value, fiber_sites);
        }
        // Native-declared store — a value passed to a native that stores it
        // (uncounted) into another argument or an external structure escapes,
        // exactly the solver's opaque-call `cross_region_refs` edge sources
        // (`regions/walk/walkrest.rs`'s `Call` arm). Keyed on the callee's declared
        // `RegionEffect`: `Stores{args}`/`Sends{args}` seed those args (the edge
        // sources); `Mixed`/`Unknown` seeds every arg (the solver's full mutual
        // clique — any arg may be stored); `Fresh`/`Immediate`/`PassThrough`/`Funnel`/
        // `Opaque` and an opaque user fn (`None`) seed nothing (no uncounted store the
        // caller must account for — `Opaque` copies every arg out, storing none). This is how `chan/send` (`Sends{[1]}`) marks its message
        // escaping while `fiber/new`/`chan/recv` (`Fresh`) do not — the spawned
        // closure rides the fresh fiber result and escapes only if that result
        // does. (`ev/spawn` is a user fn, so its closure's escape is accounted in
        // its own compilation.) Under the default classification — empty effects —
        // every callee is `None`, so this seeds nothing: an additive precision that
        // only fires when the analysis is given the real classification.
        HirKind::Call { func, args, .. } => {
            use crate::primitives::def::RegionEffect;
            match call_effect(func, arena, call_class) {
                Some(RegionEffect::Stores { args: stored }) => {
                    // Store facet — the value goes into another arg / external
                    // structure (a containment edge), not across a frontier.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, other_seeds);
                        }
                    }
                }
                Some(RegionEffect::Sends { args: stored }) => {
                    // Fiber facet (send half) — `chan/send`'s message crosses to the
                    // receiving fiber, so it is a frontier crossing (fiber seeds +
                    // region-level fiber sites), distinct from a `Stores` containment.
                    for &i in stored {
                        if let Some(a) = args.get(i) {
                            tail_sources(ctx, &a.expr, fiber_seeds);
                            record_frontier_sites(ctx, &a.expr, fiber_sites);
                        }
                    }
                }
                Some(RegionEffect::Mixed | RegionEffect::Unknown) => {
                    for a in args {
                        tail_sources(ctx, &a.expr, other_seeds);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    hir.for_each_child(|c| {
        collect_flow(
            ctx,
            c,
            call_class,
            edges,
            return_seeds,
            fiber_seeds,
            other_seeds,
            return_sites,
            fiber_sites,
            lambda_captures,
        )
    });
}
