//! Return-ownership wrapping pass for HIR.
//!
//! Runs after `mark_tail_calls` (it reads the `is_tail` flag) and after
//! `anf_lift` (it wraps the post-ANF tail value). For every function
//! body, it wraps each tail value that is **not** a tail call in a
//! `HirKind::Return` marker. `Return` lowers to a `IncrefValueRegion`:
//! it increfs the region of the result's runtime value, handing the
//! caller exactly one owning reference to whatever region the result
//! actually lives in (a fresh callee allocation, a passed-through arg,
//! or a branch-dependent mix — never named at compile time).
//!
//! This is the callee half of the prediction-free calling convention.
//! The caller balances it with a `DecrefValueRegion` at the result
//! binding's `decref_point` (see `src/lir/lower`). Tail calls are left
//! unwrapped: the inner callee already retained its result, that
//! reference propagates, and wrapping would defeat tail-call
//! optimization by moving the call out of tail position.
//!
//! The traversal mirrors `mark_tail_calls` exactly so that "tail
//! position" means the same thing in both passes. Two flags carry it and
//! they are invalidated by different things: `in_tail` by any node whose
//! child is not its result, `tail_blocks` — which makes a `break`
//! targeting a tail block a returning position — only by a **function
//! boundary**, since a break reaches its target's exit label by a jump
//! that no enclosing construct intercepts (docs/impl/region/mechanism.md
//! § "A break out of a TAIL block carries the return mint").

use std::collections::HashSet;

use super::expr::{BlockId, Hir, HirKind};

#[cfg(test)]
mod tests;

/// Wrap every function body's non-tail-call tail value in `Return`.
pub(crate) fn wrap_tail_returns(hir: &mut Hir) {
    // The top level is a returning context too. `eval_syntax` compiles a
    // macro transformer `(fn …)` as a top-level expression and hands the
    // resulting closure to the Rust macro table, which stores the `Value`
    // with no incref. With tail-region suppression gone, that closure's
    // `decref_point` `DecrefRegion` would free it before the next expansion
    // dereferences it (dangling-closure UAF in `populate_env`). Wrapping
    // the top-level tail value in `Return` retains its region (+1, never
    // released by Rust): that retain IS the transfer of ownership to the
    // macro table. For a plain script the +1 is a benign leak of the final
    // value, reclaimed at fiber teardown.
    walk(hir, true, &HashSet::new());
}

fn walk(hir: &mut Hir, in_tail: bool, tail_blocks: &HashSet<BlockId>) {
    // Phase 1: recurse, propagating tail position exactly as
    // `mark_tail_calls::mark` does. `handled` is true when the arm
    // already recursed into all children with the right tail-ness.
    let handled = match &mut hir.kind {
        // A lambda body is a fresh function boundary, always in tail
        // position; no enclosing block is reachable across it.
        HirKind::Lambda { body, .. } => {
            walk(body, true, &HashSet::new());
            true
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk(cond, false, tail_blocks);
            walk(then_branch, in_tail, tail_blocks);
            walk(else_branch, in_tail, tail_blocks);
            true
        }
        HirKind::Begin(exprs) => {
            if let Some((last, rest)) = exprs.split_last_mut() {
                for e in rest {
                    walk(e, false, tail_blocks);
                }
                walk(last, in_tail, tail_blocks);
            }
            true
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (_, v) in bindings {
                walk(v, false, tail_blocks);
            }
            walk(body, in_tail, tail_blocks);
            true
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                walk(c, false, tail_blocks);
                walk(b, in_tail, tail_blocks);
            }
            if let Some(e) = else_branch {
                walk(e, in_tail, tail_blocks);
            }
            true
        }
        HirKind::Match { value, arms } => {
            walk(value, false, tail_blocks);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    walk(g, false, tail_blocks);
                }
                walk(body, in_tail, tail_blocks);
            }
            true
        }
        // `and`/`or` are NOT tail-transparent for the return mint, unlike for
        // `mark_tail_calls`. A short-circuit value is ANY operand (the first truthy
        // for `or`, the first falsy for `and`), not just the last — and the lowerer
        // funnels every operand through one result slot. Pushing `Return` into only
        // the last operand (as the tail-transparent compounds do) leaves a
        // short-circuit-returned non-last operand with no `IncrefValueRegion`: its
        // owned-region decref then fires with no balancing mint, freeing the returned
        // value under the caller (the `(or url …)` passthrough UAF). So walk every
        // operand as NON-tail and let phase 3 wrap the WHOLE `and`/`or` in `Return`
        // (`is_wrappable` admits them): the mint lands on the result slot whichever
        // operand fills it, and `return_sites` extends every operand region's
        // `decref_point` past that mint. A tail call in the last operand keeps its
        // `is_tail` flag (set by `mark_tail_calls`) and still TCO-replaces the frame —
        // the wrapping mint sits in the unreached fall-through, inert on that path.
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs.iter_mut() {
                walk(e, false, tail_blocks);
            }
            true
        }
        HirKind::Block { block_id, body, .. } => {
            if in_tail {
                let mut child = tail_blocks.clone();
                child.insert(*block_id);
                if let Some((last, rest)) = body.split_last_mut() {
                    for e in rest {
                        walk(e, false, &child);
                    }
                    walk(last, in_tail, &child);
                }
            } else {
                for e in body {
                    walk(e, false, tail_blocks);
                }
            }
            true
        }
        HirKind::Break { block_id, value } => {
            let break_in_tail = tail_blocks.contains(block_id);
            walk(value, break_in_tail, tail_blocks);
            true
        }
        // A call's function and arguments are never in tail position.
        HirKind::Call { func, args, .. } => {
            walk(func, false, tail_blocks);
            for arg in args {
                walk(&mut arg.expr, false, tail_blocks);
            }
            true
        }
        _ => false,
    };

    // Phase 2: arms not handled above carry no tail position into any
    // child; recurse into every child as non-tail to reach nested
    // lambda bodies (e.g. a lambda bound in a `let` value, or an
    // argument lambda).
    //
    // `tail_blocks` travels through unchanged, because it and `in_tail` answer
    // different questions and only a **function boundary** — the `Lambda` arm —
    // invalidates the second. A `break` reaches its target block's exit label by
    // a jump, so an enclosing node that is not itself a tail position does not
    // sever the break's: the pervasive `(fn … (forever … (break v)))` idiom puts a
    // `Loop` between the tail block and the break, and `v` is still the
    // function's result. Reset the set here and that `v` is returned with no mint
    // while the release pinned at the block's exit label still fires, so the
    // caller reads a freed value (docs/impl/region/mechanism.md § "A break out of
    // a TAIL block carries the return mint"). `mark_tail_calls` threads the set
    // through every arm for the same reason, and the two passes must agree on
    // what "tail position" means or the mint and the release disagree.
    if !handled {
        hir.for_each_child_mut(|c| walk(c, false, tail_blocks));
    }

    // Phase 3: if this node is the value-producing leaf of a tail
    // position, mark it as a function return.
    if in_tail && is_wrappable(&hir.kind) {
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
/// `and`/`or` are NOT in this exclusion list: every operand can be the
/// short-circuit return value but the lowerer routes them through one result
/// slot, so the WHOLE compound is wrapped (the mint lands on the slot) rather
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

/// Replace `*hir` with `Return { value: <old hir> }`, preserving the
/// inner node's span and signal on the wrapper.
fn wrap_in_return(hir: &mut Hir) {
    let span = hir.span.clone();
    // Temporarily move the inner node out behind a placeholder.
    let inner = std::mem::replace(hir, Hir::silent(HirKind::Error, span.clone()));
    let signal = inner.signal;
    hir.kind = HirKind::Return {
        value: Box::new(inner),
    };
    hir.span = span;
    hir.signal = signal;
}
