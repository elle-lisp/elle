//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::{Analyzer, BindingArena};
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
        "(letrec [f (fn (& args) nil) g (fn (& args) nil) h (fn (& args) nil)] {})",
        source
    );
    let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
    let mut expander = Expander::new();
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand failed");
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
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
        | HirKind::Emit { value, .. } => {
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
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                collect_calls(init, calls);
            }
            collect_calls(body, calls);
        }
        HirKind::Recur { args } => {
            for arg in args {
                collect_calls(arg, calls);
            }
        }
        HirKind::MakeCell { value } => {
            collect_calls(value, calls);
        }
        HirKind::DerefCell { cell } => {
            collect_calls(cell, calls);
        }
        HirKind::SetCell { cell, value } => {
            collect_calls(cell, calls);
            collect_calls(value, calls);
        }
        HirKind::Intrinsic { args, .. } => {
            for arg in args {
                collect_calls(arg, calls);
            }
        }
        HirKind::Return { value } => collect_calls(value, calls),
        // Leaves: no children to recurse into
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_)
        | HirKind::Error => {}
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
    // (fn () (let [x 1] (f x))) - f is in tail position
    let hir = analyze_and_mark("(fn () (let [x 1] (f x)))");
    let calls = find_calls(&hir);
    assert_eq!(calls, vec![true]); // f is tail
}

#[test]
fn test_let_binding_not_tail() {
    // (fn () (let [x (f)] x)) - f is NOT in tail position
    let hir = analyze_and_mark("(fn () (let [x (f)] x))");
    let calls = find_calls(&hir);
    assert_eq!(calls, vec![false]); // binding value is not tail
}

#[test]
fn test_recursive_tail_call() {
    // Classic tail-recursive countdown
    let hir = analyze_and_mark("(def count-down (fn (n) (if (<= n 0) 0 (count-down (- n 1)))))");
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
    let hir = analyze_and_mark("(fn (x) (cond (= x 1) (f) (= x 2) (g) (h)))");
    let calls = find_calls(&hir);
    // Calls: = (not tail), f (tail), = (not tail), g (tail), h (tail)
    assert_eq!(calls, vec![false, true, false, true, true]);
}
