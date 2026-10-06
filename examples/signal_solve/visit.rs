// audited: 2026-10-05
//! Visit a HIR node's direct children, and find the `.lisp` files a file's literal imports name.
//!
//! docs/impl/solver.md

use elle::hir::{BindingArena, Hir, HirKind};
use elle::value::SymbolId;

/// A path in the one spelling every table keys on.
pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Resolve an import spec as `import` does, to a `.lisp` file.
pub fn resolve(spec: &str) -> Option<String> {
    elle::primitives::modules::resolve_import(spec)
        .filter(|p| p.ends_with(".lisp"))
        .map(|p| canonical(&p))
}

/// The resolved `.lisp` paths of every `(import "literal")` in a file.
pub fn literal_imports(hir: &Hir, arena: &BindingArena, out: &mut Vec<String>) {
    if let HirKind::Call { func, args, .. } = &hir.kind {
        let is_import = matches!(&func.kind, HirKind::Var(b)
            if arena.get(*b).is_primitive && arena.get(*b).name == SymbolId::of("import"));
        if is_import {
            if let Some(HirKind::String(spec)) = args.first().map(|a| &a.expr.kind) {
                if let Some(path) = resolve(spec) {
                    if !out.contains(&path) {
                        out.push(path);
                    }
                }
            }
        }
    }
    each_child(hir, &mut |c| literal_imports(c, arena, out));
}
/// Visit every direct child expression of a node.
pub fn each_child<'h>(hir: &'h Hir, f: &mut dyn FnMut(&'h Hir)) {
    match &hir.kind {
        HirKind::Let { bindings, body }
        | HirKind::Letrec { bindings, body }
        | HirKind::Loop { bindings, body } => {
            for (_, v) in bindings {
                f(v);
            }
            f(body);
        }
        HirKind::Lambda { body, .. } => f(body),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            f(cond);
            f(then_branch);
            f(else_branch);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                f(c);
                f(b);
            }
            if let Some(e) = else_branch {
                f(e);
            }
        }
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => es.iter().for_each(f),
        HirKind::Block { body, .. } => body.iter().for_each(f),
        HirKind::Break { value, .. }
        | HirKind::Return { value }
        | HirKind::Emit { value, .. }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::MakeCell { value } => f(value),
        HirKind::Call { func, args, .. } => {
            f(func);
            for a in args {
                f(&a.expr);
            }
        }
        HirKind::While { cond, body } => {
            f(cond);
            f(body);
        }
        HirKind::Recur { args } | HirKind::Intrinsic { args, .. } => args.iter().for_each(f),
        HirKind::Match { value, arms } => {
            f(value);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    f(g);
                }
                f(b);
            }
        }
        HirKind::Eval { expr, env } => {
            f(expr);
            f(env);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                f(p);
                f(v);
            }
            f(body);
        }
        HirKind::DerefCell { cell } => f(cell),
        HirKind::SetCell { cell, value } => {
            f(cell);
            f(value);
        }
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
