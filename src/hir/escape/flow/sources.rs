//! The source-collection walks: what atoms and allocation sites a
//! region-transparent expression flows to. `tail_sources` is the value-flow
//! result set (mirroring the region solver's `walk`); `return_atoms` adds the
//! assign/set-cell target seeding needed at a tail; `record_frontier_sites` is
//! the atomless region-level half. All three walk the same positions so their
//! results line up facet-for-facet.

use rustc_hash::FxHashSet;

use super::atom::Atom;
use super::TailCtx;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};

/// The atoms an expression region-transparently evaluates to — the set whose
/// regions the solver's `walk` returns for `hir`. Mirrors that walk's
/// result-flow exactly (walk.rs / walkrest.rs): a `Var`/`Lambda` is itself; a
/// binding/control form flows from its body/branches/tail; an `Eval`, allocating
/// intrinsic, loop, or immediate yields a fresh region or nothing, so no atom
/// flows out. A `Call` is opaque (fresh call-result region) *except* a tail call
/// to an arg-returning callee, which is region-transparent in the returned args
/// (the arg-return summary — see `compute_arg_return`).
//
// `pub(in crate::hir::escape)` (was private in the pre-split `flow.rs`): the
// sibling `summary` and `collect` submodules both drive this walk, so it must be
// reachable from them — the minimal widening for the split.
pub(in crate::hir::escape) fn tail_sources(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
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
pub(in crate::hir::escape) fn return_atoms(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
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
pub(in crate::hir::escape) fn record_frontier_sites(
    ctx: &TailCtx,
    hir: &Hir,
    out: &mut FxHashSet<HirId>,
) {
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
        // Fresh allocations with no atom: a string/quoted-compound literal, an
        // `Eval` result region, or an `Emit`'s — what the resumer handed back.
        HirKind::String(_)
        | HirKind::QuoteConst(_)
        | HirKind::Eval { .. }
        | HirKind::Emit { .. } => {
            out.insert(hir.id);
        }
        // Immediates and no-value forms (`While`/`Break`/`Recur`): no region.
        _ => {}
    }
}
