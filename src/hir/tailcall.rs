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
pub fn mark_tail_calls(hir: &mut Hir) {
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

        // Yield: value is not in tail position
        HirKind::Yield(expr) => {
            mark(expr, false, tail_blocks);
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

        // Leaves: nothing to recurse into
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::Analyzer;
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    /// Wrap source in a letrec that pre-defines common free variables
    /// (f, g, h) so they resolve during analysis. The tail-call tests
    /// use these as placeholder callees.
    fn analyze_and_mark(source: &str) -> Hir {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);

        // Wrap in letrec to define free variables used by tail-call tests
        let wrapped = format!(
            "(letrec ((f (fn (& args) nil)) (g (fn (& args) nil)) (h (fn (& args) nil))) {})",
            source
        );
        let syntax = read_syntax(&wrapped).expect("parse failed");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand failed");
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        mark_tail_calls(&mut analysis.hir);
        analysis.hir
    }

    fn find_calls(hir: &Hir) -> Vec<bool> {
        let mut calls = Vec::new();
        collect_calls(hir, &mut calls);
        calls
    }

    fn collect_calls(hir: &Hir, calls: &mut Vec<bool>) {
        match &hir.kind {
            HirKind::Call {
                is_tail,
                func,
                args,
            } => {
                calls.push(*is_tail);
                collect_calls(func, calls);
                for arg in args {
                    collect_calls(&arg.expr, calls);
                }
            }
            HirKind::Lambda { body, .. } => collect_calls(body, calls),
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_calls(cond, calls);
                collect_calls(then_branch, calls);
                collect_calls(else_branch, calls);
            }
            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
                for expr in exprs {
                    collect_calls(expr, calls);
                }
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, value) in bindings {
                    collect_calls(value, calls);
                }
                collect_calls(body, calls);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (cond, body) in clauses {
                    collect_calls(cond, calls);
                    collect_calls(body, calls);
                }
                if let Some(else_br) = else_branch {
                    collect_calls(else_br, calls);
                }
            }
            HirKind::Match { value, arms } => {
                collect_calls(value, calls);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        collect_calls(g, calls);
                    }
                    collect_calls(body, calls);
                }
            }
            HirKind::While { cond, body } => {
                collect_calls(cond, calls);
                collect_calls(body, calls);
            }
            HirKind::Assign { value, .. }
            | HirKind::Define { value, .. }
            | HirKind::Destructure { value, .. }
            | HirKind::Yield(value) => {
                collect_calls(value, calls);
            }
            HirKind::Eval { expr, env } => {
                collect_calls(expr, calls);
                collect_calls(env, calls);
            }
            HirKind::Parameterize { bindings, body } => {
                for (param, value) in bindings {
                    collect_calls(param, calls);
                    collect_calls(value, calls);
                }
                collect_calls(body, calls);
            }
            HirKind::Block { body, .. } => {
                for expr in body {
                    collect_calls(expr, calls);
                }
            }
            HirKind::Break { value, .. } => {
                collect_calls(value, calls);
            }
            _ => {}
        }
    }

    #[test]
    fn test_simple_tail_call() {
        // (fn (x) (f x)) - the call to f is in tail position
        let hir = analyze_and_mark("(fn (x) (f x))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![true]); // f is tail call
    }

    #[test]
    fn test_non_tail_call_in_addition() {
        // (fn (x) (+ (f x) 1)) - the call to f is NOT in tail position
        let hir = analyze_and_mark("(fn (x) (+ (f x) 1))");
        let calls = find_calls(&hir);
        // First call is +, second is f - both are not tail (+ is outer, f is arg)
        assert_eq!(calls, vec![true, false]); // + is tail, f is not
    }

    #[test]
    fn test_if_branches_tail() {
        // (fn (x) (if x (f 1) (g 2))) - both f and g are in tail position
        let hir = analyze_and_mark("(fn (x) (if x (f 1) (g 2)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![true, true]); // both branches are tail
    }

    #[test]
    fn test_if_condition_not_tail() {
        // (fn (x) (if (f x) 1 2)) - f is NOT in tail position
        let hir = analyze_and_mark("(fn (x) (if (f x) 1 2))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false]); // condition is not tail
    }

    #[test]
    fn test_begin_last_is_tail() {
        // (fn () (begin (f) (g))) - f is not tail, g is tail
        let hir = analyze_and_mark("(fn () (begin (f) (g)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false, true]); // f not tail, g is tail
    }

    #[test]
    fn test_let_body_is_tail() {
        // (fn () (let ((x 1)) (f x))) - f is in tail position
        let hir = analyze_and_mark("(fn () (let ((x 1)) (f x)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![true]); // f is tail
    }

    #[test]
    fn test_let_binding_not_tail() {
        // (fn () (let ((x (f))) x)) - f is NOT in tail position
        let hir = analyze_and_mark("(fn () (let ((x (f))) x))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false]); // binding value is not tail
    }

    #[test]
    fn test_recursive_tail_call() {
        // Classic tail-recursive countdown
        let hir =
            analyze_and_mark("(def count-down (fn (n) (if (<= n 0) 0 (count-down (- n 1)))))");
        let calls = find_calls(&hir);
        // Calls: <=, -, count-down
        // <= is in condition (not tail), - is arg (not tail), count-down is tail
        assert_eq!(calls, vec![false, true, false]);
    }

    #[test]
    fn test_top_level_not_tail() {
        // Top-level call is not in tail position (not inside a lambda)
        let hir = analyze_and_mark("(f 1)");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false]); // top-level is not tail
    }

    #[test]
    fn test_nested_lambda_tail() {
        // (fn () ((fn () (f)))) - inner f is tail in inner lambda
        let hir = analyze_and_mark("(fn () ((fn () (f))))");
        let calls = find_calls(&hir);
        // Outer call to inner lambda is tail, inner call to f is tail
        assert_eq!(calls, vec![true, true]);
    }

    #[test]
    fn test_and_last_is_tail() {
        // (fn () (and (f) (g))) - f is not tail, g is tail
        let hir = analyze_and_mark("(fn () (and (f) (g)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false, true]);
    }

    #[test]
    fn test_or_last_is_tail() {
        // (fn () (or (f) (g))) - f is not tail, g is tail
        let hir = analyze_and_mark("(fn () (or (f) (g)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false, true]);
    }

    #[test]
    fn test_while_body_not_tail() {
        // (fn () (while true (f))) - f is NOT in tail position (loop body)
        let hir = analyze_and_mark("(fn () (while true (f)))");
        let calls = find_calls(&hir);
        assert_eq!(calls, vec![false]); // loop body is not tail
    }

    #[test]
    fn test_cond_bodies_are_tail() {
        // (fn (x) (cond ((= x 1) (f)) ((= x 2) (g)) (else (h))))
        let hir = analyze_and_mark("(fn (x) (cond ((= x 1) (f)) ((= x 2) (g)) (else (h))))");
        let calls = find_calls(&hir);
        // Calls: = (not tail), f (tail), = (not tail), g (tail), h (tail)
        assert_eq!(calls, vec![false, true, false, true, true]);
    }
}
