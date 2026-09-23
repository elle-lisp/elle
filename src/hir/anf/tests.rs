// audited: 2026-09-22
// The fixture the ANF pins share: a compile to post-ANF HIR, and the walks that
// ask whether a given node came out of the pass named.
//
// src/hir/anf.rs

use super::*;
use crate::hir::expr::{HirId, HirKind};
use crate::hir::testkit::HirFixture;
use crate::hir::BindingArena;
use crate::symbol::SymbolTable;

/// These tests also call `h`, and want a `cond_var` that reads as true. The
/// analyzer does not inline letrec-bound closures, so calls to these survive
/// into HIR as real `Call` nodes — which is what the ANF tests examine.
const STUBS: &str = "cond_var (fn () true) \
                     f (fn (& args) args) \
                     g (fn (& args) args) \
                     h (fn (& args) args)";

fn analyze_anf(source: &str) -> (Hir, BindingArena, SymbolTable) {
    HirFixture::new().stubs(STUBS).build(source)
}

/// Pre-order walk that hands out borrowed references with the
/// SAME lifetime as the input — unlike `Hir::for_each_child`,
/// whose `FnMut(&Hir)` is higher-rank and can't let the borrow
/// escape. We pay the verbosity tax to get correct lifetimes.
fn walk_pre<'a, F: FnMut(&'a Hir)>(hir: &'a Hir, f: &mut F) {
    f(hir);
    match &hir.kind {
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
        HirKind::Let { bindings, body }
        | HirKind::Letrec { bindings, body }
        | HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                walk_pre(init, f);
            }
            walk_pre(body, f);
        }
        HirKind::Lambda { body, .. } => walk_pre(body, f),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_pre(cond, f);
            walk_pre(then_branch, f);
            walk_pre(else_branch, f);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                walk_pre(c, f);
                walk_pre(b, f);
            }
            if let Some(eb) = else_branch {
                walk_pre(eb, f);
            }
        }
        HirKind::Begin(es) => {
            for e in es {
                walk_pre(e, f);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                walk_pre(e, f);
            }
        }
        HirKind::Break { value, .. } => walk_pre(value, f),
        HirKind::Call { func, args, .. } => {
            walk_pre(func, f);
            for a in args {
                walk_pre(&a.expr, f);
            }
        }
        HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::MakeCell { value } => walk_pre(value, f),
        HirKind::DerefCell { cell } => walk_pre(cell, f),
        HirKind::SetCell { cell, value } => {
            walk_pre(cell, f);
            walk_pre(value, f);
        }
        HirKind::While { cond, body } => {
            walk_pre(cond, f);
            walk_pre(body, f);
        }
        HirKind::Recur { args } => {
            for a in args {
                walk_pre(a, f);
            }
        }
        HirKind::And(es) | HirKind::Or(es) => {
            for e in es {
                walk_pre(e, f);
            }
        }
        HirKind::Emit { value, .. } => walk_pre(value, f),
        HirKind::Return { value } => walk_pre(value, f),
        HirKind::Match { value, arms } => {
            walk_pre(value, f);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    walk_pre(g, f);
                }
                walk_pre(body, f);
            }
        }
        HirKind::Destructure { value, .. } => walk_pre(value, f),
        HirKind::Eval { expr, env } => {
            walk_pre(expr, f);
            walk_pre(env, f);
        }
        HirKind::Parameterize { bindings, body } => {
            for (k, v) in bindings {
                walk_pre(k, f);
                walk_pre(v, f);
            }
            walk_pre(body, f);
        }
        HirKind::Intrinsic { args, .. } => {
            for a in args {
                walk_pre(a, f);
            }
        }
    }
}

/// Strip `DerefCell` wrappers introduced by `functionalize` for
/// needs-capture bindings. `Var(g)` becomes `DerefCell(Var(g))`
/// when `g` is captured by a nested closure; tests address the
/// inner `Var`.
fn unwrap_deref_cell(hir: &Hir) -> &Hir {
    if let HirKind::DerefCell { cell } = &hir.kind {
        cell
    } else {
        hir
    }
}

/// Find every Call whose `func` is `Var(name)` (resolved via
/// symbols), looking through `DerefCell`.
fn find_calls_to<'a>(
    hir: &'a Hir,
    name: &str,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> Vec<&'a Hir> {
    let mut out = Vec::new();
    let mut visit = |node: &'a Hir| {
        if let HirKind::Call { func, .. } = &node.kind {
            let func_ref = unwrap_deref_cell(func);
            if let HirKind::Var(b) = &func_ref.kind {
                if symbols.name(arena.get(*b).name) == Some(name) {
                    out.push(node);
                }
            }
        }
    };
    walk_pre(hir, &mut visit);
    out
}

/// HirId-based lookup helper.
#[allow(dead_code)]
fn find_node<'a>(hir: &'a Hir, target: HirId) -> Option<&'a Hir> {
    let mut found: Option<&'a Hir> = None;
    let mut visit = |node: &'a Hir| {
        if found.is_none() && node.id == target {
            found = Some(node);
        }
    };
    walk_pre(hir, &mut visit);
    found
}

/// True if `hir` is `(let [b e] (var b))` — the ANF wrap shape — or, at a
/// returning position, `(let [b e] (return (var b)))`: `wrap_tail_returns`
/// runs inside `anf_lift` and marks the wrap's body as the returned value.
fn is_anf_wrap(hir: &Hir) -> bool {
    if let HirKind::Let { bindings, body } = &hir.kind {
        if bindings.len() == 1 {
            let (b, _) = &bindings[0];
            let returned = match &body.kind {
                HirKind::Return { value } => value.as_ref(),
                _ => body.as_ref(),
            };
            if let HirKind::Var(bv) = &returned.kind {
                return bv == b;
            }
        }
    }
    false
}

/// Extract the init expression from an ANF wrap. Panics if not a wrap.
fn anf_wrap_init(hir: &Hir) -> &Hir {
    match &hir.kind {
        HirKind::Let { bindings, .. } if bindings.len() == 1 => &bindings[0].1,
        _ => panic!("expected ANF wrap"),
    }
}

/// The id of every node an ANF wrap names, anywhere in the tree. A position
/// test that reads the shape from the outside in has to know which form the
/// front end left around the node; this asks the one question the naming rule
/// is about — did this node get a binding of its own?
fn named_node_ids(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    let mut visit = |node: &Hir| {
        if is_anf_wrap(node) {
            out.push(anf_wrap_init(node).id);
        }
    };
    walk_pre(hir, &mut visit);
    out
}

/// The single call to `name`, which the naming tests then ask about.
fn only_call_to<'a>(
    hir: &'a Hir,
    name: &str,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> &'a Hir {
    let calls = find_calls_to(hir, name, arena, symbols);
    assert_eq!(calls.len(), 1, "expected exactly one call to {name}");
    calls[0]
}

mod positions;
mod tails;
