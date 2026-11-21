// audited: 2026-11-20
//! Wraps each tail value of a function body that is not a tail call in `HirKind::Return`, the return mint.
//!
//! The pass runs after `mark_tail_calls`, whose `is_tail` flags it reads, and
//! after `anf_lift`, whose tail values it wraps. `Return` lowers to
//! `IncrefValueRegion`, which hands the caller one owning reference to
//! whatever region the result lives in. The caller releases it at the result
//! binding's `decref_point`. A tail call stays unwrapped: its callee already
//! retained the result, and a wrapper would move the call out of tail position.
//!
//! One read-only walk decides tail position, and two readers share it. The
//! wrap reads it after ANF. The lowerer reads it on the final tree, for each
//! marked tail call the walk reaches outside a tail position. Functionalization
//! and the walk's `and`/`or` rule put a marked call there, and a `Return` node
//! then covers its value. Two flags carry tail position. Any node whose child
//! is not its result clears `in_tail`; only a function boundary clears
//! `tail_blocks`, the set of blocks a `break` returns through.
//!
//! docs/impl/region/returnmint.md
//! docs/impl/region/anchors.md

use std::collections::HashSet;

use rustc_hash::FxHashSet;

use super::expr::{BlockId, Hir, HirId, HirKind};

#[cfg(test)]
mod tests;

/// Wrap every function body's non-tail-call tail value in `Return`.
pub(crate) fn wrap_tail_returns(hir: &mut Hir) {
    // The top level is a returning context too. `eval_syntax` compiles a
    // macro transformer `(fn …)` as a top-level expression and hands the
    // resulting closure to the Rust macro table, which stores the `Value`
    // with no incref. Unretained, that closure's `decref_point`
    // `DecrefRegion` would free it before the next expansion dereferences
    // it. Wrapping the top-level tail value in `Return` retains its region
    // (+1, never released by Rust): that retain IS the transfer of
    // ownership to the macro table. For a script, the +1 is the program
    // value's reference, which the host gives back
    // (docs/impl/region/rules.md).
    let mut leaves = FxHashSet::default();
    walk(hir, true, &HashSet::new(), &mut |node, in_tail| {
        if in_tail && is_wrappable(&node.kind) {
            leaves.insert(node.id);
        }
    });
    wrap_each(hir, &leaves);
}

/// Every tail call whose value a `Return` node mints: each
/// `Call { is_tail: true }` the walk reaches outside a tail position. The
/// lowerer stands down such a call's fall-through retain, so the value takes
/// one mint (docs/impl/region/returnmint.md).
pub(crate) fn return_minted_tail_calls(hir: &Hir) -> FxHashSet<HirId> {
    let mut calls = FxHashSet::default();
    walk(hir, true, &HashSet::new(), &mut |node, in_tail| {
        if !in_tail && matches!(node.kind, HirKind::Call { is_tail: true, .. }) {
            calls.insert(node.id);
        }
    });
    calls
}

/// Report each node with whether it is in tail position, propagating tail
/// position as `mark_tail_calls` does, except for `and`/`or`.
fn walk<F: FnMut(&Hir, bool)>(
    hir: &Hir,
    in_tail: bool,
    tail_blocks: &HashSet<BlockId>,
    report: &mut F,
) {
    report(hir, in_tail);
    match &hir.kind {
        // A lambda body is a fresh function boundary, always in tail
        // position; no enclosing block is reachable across it.
        HirKind::Lambda(lambda) => walk(&lambda.body, true, &HashSet::new(), report),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk(cond, false, tail_blocks, report);
            walk(then_branch, in_tail, tail_blocks, report);
            walk(else_branch, in_tail, tail_blocks, report);
        }
        HirKind::Begin(exprs) => {
            if let Some((last, rest)) = exprs.split_last() {
                for e in rest {
                    walk(e, false, tail_blocks, report);
                }
                walk(last, in_tail, tail_blocks, report);
            }
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (_, v) in bindings {
                walk(v, false, tail_blocks, report);
            }
            walk(body, in_tail, tail_blocks, report);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                walk(c, false, tail_blocks, report);
                walk(b, in_tail, tail_blocks, report);
            }
            if let Some(e) = else_branch {
                walk(e, in_tail, tail_blocks, report);
            }
        }
        HirKind::Match { value, arms } => {
            walk(value, false, tail_blocks, report);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    walk(g, false, tail_blocks, report);
                }
                walk(body, in_tail, tail_blocks, report);
            }
        }
        // `and`/`or` are not tail-transparent for the return mint, unlike for
        // `mark_tail_calls`. Any operand can be the result (the first truthy
        // for `or`, the first falsy for `and`), and the lowerer funnels every
        // operand through one result slot. A `Return` in the last operand alone
        // would leave a short-circuit result with no mint, while its region's
        // release still fires. So walk every operand as non-tail, and let the
        // wrap take the whole `and`/`or`: the mint lands on the result slot
        // whichever operand fills it, and `return_sites` extends every operand
        // region's `decref_point` past that mint. A tail call in the last
        // operand keeps its `is_tail` flag and still replaces the frame; the
        // wrapping mint sits in the fall-through that path never reaches.
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                walk(e, false, tail_blocks, report);
            }
        }
        HirKind::Block { block_id, body, .. } => {
            if in_tail {
                let mut child = tail_blocks.clone();
                child.insert(*block_id);
                if let Some((last, rest)) = body.split_last() {
                    for e in rest {
                        walk(e, false, &child, report);
                    }
                    walk(last, in_tail, &child, report);
                }
            } else {
                for e in body {
                    walk(e, false, tail_blocks, report);
                }
            }
        }
        HirKind::Break { block_id, value } => {
            let break_in_tail = tail_blocks.contains(block_id);
            walk(value, break_in_tail, tail_blocks, report);
        }
        // Every other node carries no tail position into any child: a call's
        // function and arguments, a `Return`'s value, a binding's value. The
        // walk still reaches nested lambda bodies, such as a lambda bound in a
        // `let` value, or an argument lambda.
        //
        // `tail_blocks` travels through unchanged: only a function boundary, the
        // `Lambda` arm, clears it. In `(fn … (forever … (break v)))` a `Loop` sits
        // between the tail block and the break, and `v` is still the function's
        // result, so it still needs its mint.
        _ => hir.for_each_child(|c| walk(c, false, tail_blocks, report)),
    }
}

/// Wrap each node whose id is in `leaves`, children first.
fn wrap_each(hir: &mut Hir, leaves: &FxHashSet<HirId>) {
    hir.for_each_child_mut(|c| wrap_each(c, leaves));
    if leaves.contains(&hir.id) {
        wrap_in_return(hir);
    }
}

/// Whether a node in tail position is a value-producing leaf that
/// should carry the `Return` marker. The tail-transparent compounds
/// propagate tail position to their children (which get wrapped
/// individually), so the compound itself is not wrapped. A tail call is
/// not wrapped (TCO + the inner callee already retained). `Recur` is a
/// loop back-edge, not a returned value.
///
/// `and`/`or` are NOT in this exclusion list. Every operand can be the
/// short-circuit return value, but the lowerer routes them through one result
/// slot. So the WHOLE compound is wrapped (the mint lands on the slot) rather
/// than each operand individually — see the `And`/`Or` walk arm.
fn is_wrappable(kind: &HirKind) -> bool {
    !matches!(
        kind,
        HirKind::If { .. }
            | HirKind::Begin(_)
            | HirKind::Let { .. }
            | HirKind::Letrec { .. }
            | HirKind::Cond { .. }
            | HirKind::Match { .. }
            | HirKind::Block { .. }
            | HirKind::Break { .. }
            | HirKind::Call { is_tail: true, .. }
            | HirKind::Recur { .. }
            | HirKind::Return { .. }
            | HirKind::Error
    )
}

/// Replace `*hir` with `Return { value: <old hir> }`. The inner node keeps its
/// id, and the wrapper takes a fresh one with the inner node's span and signal.
fn wrap_in_return(hir: &mut Hir) {
    let span = hir.span;
    // Temporarily move the inner node out behind a placeholder.
    let inner = std::mem::replace(hir, Hir::silent(HirKind::Error, span));
    let signal = inner.signal;
    hir.kind = HirKind::Return {
        value: Box::new(inner),
    };
    hir.span = span;
    hir.signal = signal;
}
