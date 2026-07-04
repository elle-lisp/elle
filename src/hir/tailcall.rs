//! Tail call marking pass for HIR
//!
//! This pass walks the HIR tree after analysis and marks `Call` nodes
//! that are in tail position with `is_tail: true`. A call is in tail
//! position if its result is immediately returned from the enclosing
//! lambda without further computation.
//!
//! Tail position is defined recursively:
//! - The body of a lambda is in tail position
//! - The last expression of a `begin` inherits tail position
//! - Both branches of an `if` inherit tail position
//! - The body of `let`/`letrec` inherits tail position
//! - Clause bodies and else branch of `cond` inherit tail position
//! - Arm bodies of `match` inherit tail position
//! - Handler bodies of `handler-case` inherit tail position (but NOT the protected body)
//! - The last expression of `and`/`or` inherits tail position
//! - The body of `block` inherits tail position (last expression)
//!
//! NOT in tail position:
//! - Conditions of `if`, `cond`, `while`
//! - Arguments to calls
//! - Function position of calls
//! - Value expressions in bindings
//! - Loop bodies (`while`)
//! - `throw` value, `yield` value
//! - Match scrutinee and guards

use std::collections::HashSet;

use super::expr::{BlockId, Hir, HirKind};

/// Mark tail calls in a HIR tree.
///
/// Call this after analysis, before lowering. The pass walks the tree
/// and sets `is_tail: true` on `Call` nodes that are in tail position.
pub(crate) fn mark_tail_calls(hir: &mut Hir) {
    // Top-level expressions are not inside a lambda, so not in tail position
    mark(hir, false, &HashSet::new());
}

/// Recursively mark tail calls in a HIR node.
///
/// `in_tail` indicates whether this node is in tail position.
/// `tail_blocks` tracks which `BlockId`s are in tail position, so that
/// `break` targeting one of these blocks can mark its value as tail.
fn mark(hir: &mut Hir, in_tail: bool, tail_blocks: &HashSet<BlockId>) {
    match &mut hir.kind {
        // Lambda body is always in tail position.
        // Reset tail_blocks — a new function boundary means no enclosing
        // blocks are reachable via tail call.
        HirKind::Lambda { body, .. } => {
            mark(body, true, &HashSet::new());
        }

        // Call: mark as tail if in tail position, recurse into func/args
        HirKind::Call {
            is_tail,
            func,
            args,
        } => {
            *is_tail = in_tail;
            // Function and arguments are NOT in tail position
            mark(func, false, tail_blocks);
            for arg in args {
                mark(&mut arg.expr, false, tail_blocks);
            }
        }

        // If: condition is not tail, both branches inherit tail position
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            mark(cond, false, tail_blocks);
            mark(then_branch, in_tail, tail_blocks);
            mark(else_branch, in_tail, tail_blocks);
        }

        // Begin: only the last expression inherits tail position
        HirKind::Begin(exprs) => {
            if let Some((last, rest)) = exprs.split_last_mut() {
                for expr in rest {
                    mark(expr, false, tail_blocks);
                }
                mark(last, in_tail, tail_blocks);
            }
        }

        // Let/Letrec: binding values are not tail, body inherits tail position
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (_, value) in bindings {
                mark(value, false, tail_blocks);
            }
            mark(body, in_tail, tail_blocks);
        }

        // Cond: conditions are not tail, clause bodies and else inherit tail position
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (cond, body) in clauses {
                mark(cond, false, tail_blocks);
                mark(body, in_tail, tail_blocks);
            }
            if let Some(else_br) = else_branch {
                mark(else_br, in_tail, tail_blocks);
            }
        }

        // Match: scrutinee and guards are not tail, arm bodies inherit tail position
        HirKind::Match { value, arms } => {
            mark(value, false, tail_blocks);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    mark(g, false, tail_blocks);
                }
                mark(body, in_tail, tail_blocks);
            }
        }

        // And/Or: only the last expression inherits tail position
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            if let Some((last, rest)) = exprs.split_last_mut() {
                for expr in rest {
                    mark(expr, false, tail_blocks);
                }
                mark(last, in_tail, tail_blocks);
            }
        }

        // Block: when in tail position, the last expression inherits tail
        // and the block's ID is added to tail_blocks so that `break`
        // targeting this block can also mark its value as tail.
        HirKind::Block { block_id, body, .. } => {
            if in_tail {
                let mut child_tail_blocks = tail_blocks.clone();
                child_tail_blocks.insert(*block_id);
                if let Some((last, rest)) = body.split_last_mut() {
                    for expr in rest {
                        mark(expr, false, &child_tail_blocks);
                    }
                    mark(last, in_tail, &child_tail_blocks);
                }
            } else {
                for expr in body {
                    mark(expr, false, tail_blocks);
                }
            }
        }

        // Break: value is in tail position if the target block is in
        // tail_blocks (i.e., the block itself is in tail position).
        HirKind::Break { block_id, value } => {
            let break_in_tail = tail_blocks.contains(block_id);
            mark(value, break_in_tail, tail_blocks);
        }

        // While: loop bodies are never in tail position
        HirKind::While { cond, body } => {
            mark(cond, false, tail_blocks);
            mark(body, false, tail_blocks);
        }

        // Loop: binding inits and body are never in tail position
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                mark(init, false, tail_blocks);
            }
            mark(body, false, tail_blocks);
        }

        // Recur: args are not in tail position
        HirKind::Recur { args } => {
            for arg in args {
                mark(arg, false, tail_blocks);
            }
        }

        // Assign: value is not in tail position
        HirKind::Assign { value, .. } => {
            mark(value, false, tail_blocks);
        }

        // Define: value is not in tail position
        HirKind::Define { value, .. } => {
            mark(value, false, tail_blocks);
        }

        // Destructure: value is not in tail position
        HirKind::Destructure { value, .. } => {
            mark(value, false, tail_blocks);
        }

        // Emit: value is not in tail position
        HirKind::Emit { value: expr, .. } => {
            mark(expr, false, tail_blocks);
        }

        // Return is inserted after this pass; defensive — its value
        // inherits the Return's tail position.
        HirKind::Return { value } => {
            mark(value, in_tail, tail_blocks);
        }

        // Eval: neither expr nor env is in tail position (runs in its own context)
        HirKind::Eval { expr, env } => {
            mark(expr, false, tail_blocks);
            mark(env, false, tail_blocks);
        }

        // Parameterize: bindings are not tail, body is NOT tail
        // (PopParamFrame must execute after body completes)
        HirKind::Parameterize { bindings, body } => {
            for (param, value) in bindings {
                mark(param, false, tail_blocks);
                mark(value, false, tail_blocks);
            }
            mark(body, false, tail_blocks);
        }

        // MakeCell/DerefCell/SetCell: children are not in tail position
        HirKind::MakeCell { value } => {
            mark(value, false, tail_blocks);
        }
        HirKind::DerefCell { cell } => {
            mark(cell, false, tail_blocks);
        }
        HirKind::SetCell { cell, value } => {
            mark(cell, false, tail_blocks);
            mark(value, false, tail_blocks);
        }

        // Intrinsic: args are not in tail position
        HirKind::Intrinsic { args, .. } => {
            for arg in args {
                mark(arg, false, tail_blocks);
            }
        }

        // Leaves: nothing to recurse into
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_) => {}

        HirKind::Error => {}
    }
}

#[cfg(test)]
mod tests;
