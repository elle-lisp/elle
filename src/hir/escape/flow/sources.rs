// audited: 2026-10-06
//! The source-collection walks: what atoms and allocation sites a region-transparent expression flows to.
//!
//! docs/impl/escape.md
//!
//! Every walk here descends the same positions, [`for_each_tail`], so their
//! results line up facet for facet. `tail_sources` is the value-flow result set
//! (mirroring the region solver's `walk`); `return_atoms` adds the
//! assign/set-cell target seeding needed at a tail; `record_frontier_sites` is
//! the atomless region-level half; `tail_pair_operands` names what a `%pair` the
//! expression may be holds.

use rustc_hash::FxHashSet;

use super::atom::Atom;
use super::TailCtx;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};

/// Visit each sub-expression whose value `hir` may hand up unchanged, and report
/// whether `hir` is wholly such a form.
///
/// A binding or control form hands up its body, its branches or its tail;
/// `%get` hands up an element of arg 0's region; a cell or value wrapper hands up
/// the wrapped value. A call to an arg-returning callee hands up the args it
/// returns (the arg-return summary — see `compute_arg_return`), mirroring the
/// solver's `try_inline_call` with its immutable, unmutated guard. Such a call
/// also produces a result of its own, so it reports `false` after the visit.
/// Every other node produces its value itself and reports `false`.
pub(in crate::hir::escape) fn for_each_tail<'h>(
    ctx: &TailCtx,
    hir: &'h Hir,
    mut visit: impl FnMut(&'h Hir),
) -> bool {
    match &hir.kind {
        HirKind::Let { body, .. }
        | HirKind::Letrec { body, .. }
        | HirKind::Loop { body, .. }
        | HirKind::Parameterize { body, .. } => visit(body),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            visit(then_branch);
            visit(else_branch);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (_, b) in clauses {
                visit(b);
            }
            if let Some(eb) = else_branch {
                visit(eb);
            }
        }
        HirKind::Begin(exprs) | HirKind::Block { body: exprs, .. } => {
            if let Some(last) = exprs.last() {
                visit(last);
            }
        }
        // And/Or short-circuit: any operand can be the result (the solver unions
        // all of them).
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                visit(e);
            }
        }
        HirKind::Match { arms, .. } => {
            for (_, _, body) in arms {
                visit(body);
            }
        }
        HirKind::Return { value }
        | HirKind::MakeCell { value }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::SetCell { value, .. } => visit(value),
        HirKind::DerefCell { cell } => visit(cell),
        HirKind::Intrinsic {
            op: IntrinsicOp::Get,
            args,
        } => {
            if let Some(a0) = args.first() {
                visit(a0);
            }
        }
        HirKind::Call { func, args, .. } => {
            if let HirKind::Var(b) = &func.kind {
                let bi = ctx.arena.get(*b);
                if bi.is_immutable && !bi.is_mutated {
                    for &i in ctx.arg_return.get(b).into_iter().flatten() {
                        if let Some(a) = args.get(i) {
                            visit(&a.expr);
                        }
                    }
                }
            }
            return false;
        }
        _ => return false,
    }
    true
}

/// The atoms an expression region-transparently evaluates to — the set whose
/// regions the solver's `walk` returns for `hir`. A `Var`/`Lambda` is itself;
/// every transparent form flows from what [`for_each_tail`] visits; an `Eval`,
/// an allocating intrinsic, a `while`, or an immediate yields a fresh region or
/// nothing, so no atom flows out. A `Call` is opaque (a fresh call-result region)
/// except in the args an arg-returning callee hands back.
pub(in crate::hir::escape) fn tail_sources(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
    match &hir.kind {
        HirKind::Var(b) => out.push(Atom::Binding(*b)),
        HirKind::Lambda { .. } => out.push(Atom::Lambda(hir.id)),
        _ => {
            for_each_tail(ctx, hir, |t| tail_sources(ctx, t, out));
        }
    }
}

/// Atoms in **return position** — like `tail_sources`, but at an `assign` /
/// `set-cell!` that is itself a tail it ALSO seeds the *target* binding.
///
/// An `(assign b v)` expression evaluates to the value it just stored. At a
/// function's tail the function returns that value, and `b` now holds a returned
/// value. The solver folds the value's regions into `binding_source_regions[b]`,
/// so `(assign b (foo))` at a tail leaves `b` holding a returned region, although
/// `(foo)` is atomless. Seeding the target lands `b` in the return facet, so the
/// projection through `binding_source_regions[b]` finds that returned region.
///
/// Used ONLY for return-facet seeds. Edge collection keeps `tail_sources` (an
/// assign's *result* is its value, not the cell — `(let [x (assign c v)] …)` binds
/// `x` to `v`, never aliases the cell `c`).
pub(in crate::hir::escape) fn return_atoms(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
    match &hir.kind {
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
        // Identical to the value-flow result.
        HirKind::Var(_)
        | HirKind::Lambda { .. }
        | HirKind::Intrinsic { .. }
        | HirKind::Call { .. }
        | HirKind::Define { .. }
        | HirKind::Destructure { .. } => tail_sources(ctx, hir, out),
        _ => {
            for_each_tail(ctx, hir, |t| return_atoms(ctx, t, out));
        }
    }
}

/// The **allocation-site `HirId`s** a value flows to a frontier (tail/return,
/// emit, send) through: the region-level half of the frontier facets, recorded
/// at the same positions `tail_sources` walks. The consumer projects an *atom*
/// (a `Var` or `Lambda`) through `binding_source_regions` or `alloc_region`.
/// This walk records the *atomless* allocations no atom names: a `(%pair …)`,
/// an `(@array …)`, a call result or a string literal in frontier position, as
/// in `(yield (%pair 1 2))`. The region solver projects these through its
/// `alloc_region` map, where a `HirId` with no allocation contributes no region.
///
/// The set is an **over-approximation in the sound direction**. A `Lambda` and
/// every `Call` reached in frontier position are recorded, even an arg-returning
/// call whose descended arg also carries the flow. The consumers want a SUPERSET
/// of the escaping regions. The ownership Shared seed must never *miss* an escape
/// (`ownership/seeds.rs`), and the merge and branch-compensation gates read this
/// as an *exclusion*, where over-excluding is leak-safe.
///
/// A recorded `HirId` whose region is a runtime placeholder, such as an immediate
/// call result, is already `not_ownable` or a non-candidate downstream, so
/// projecting it changes no decision. `%get` stays transparent, because the
/// borrowed element lives in arg 0's region, as in `tail_sources`.
pub(in crate::hir::escape) fn record_frontier_sites(
    ctx: &TailCtx,
    hir: &Hir,
    out: &mut FxHashSet<HirId>,
) {
    // A binding is an atom — projected via `binding_source_regions`, not here.
    if matches!(hir.kind, HirKind::Var(_)) {
        return;
    }
    if for_each_tail(ctx, hir, |t| record_frontier_sites(ctx, t, out)) {
        return;
    }
    match &hir.kind {
        // A lambda's closure region, a call's result region, an allocating
        // intrinsic's region, and the fresh allocations with no atom: a string or
        // quoted-compound literal, an `Eval` result, or an `Emit`'s — what the
        // resumer handed back. A non-allocating intrinsic returns an immediate,
        // absent from `alloc_region`, so recording it is harmless.
        HirKind::Lambda { .. }
        | HirKind::Call { .. }
        | HirKind::Intrinsic { .. }
        | HirKind::String(_)
        | HirKind::QuoteConst(_)
        | HirKind::Eval { .. }
        | HirKind::Emit { .. } => {
            out.insert(hir.id);
        }
        // Immediates and no-value forms (`While`/`Break`/`Recur`): no region.
        _ => {}
    }
}

/// The atoms stored into every `%pair` that `hir` may evaluate to. A pair holds
/// each operand by the counted edge its allocation takes, so what it stores
/// follows the pair into the containment facet, as a closure's captures follow
/// the closure (docs/impl/escape.md).
pub(in crate::hir::escape) fn tail_pair_operands(ctx: &TailCtx, hir: &Hir, out: &mut Vec<Atom>) {
    if let HirKind::Intrinsic {
        op: IntrinsicOp::Pair,
        args,
    } = &hir.kind
    {
        for a in args {
            tail_sources(ctx, a, out);
        }
        return;
    }
    for_each_tail(ctx, hir, |t| tail_pair_operands(ctx, t, out));
}
