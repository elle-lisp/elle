use super::*;

impl Hir {
    /// Iterate over the immediate child HIR nodes of this node.
    /// Visit each child mutably, in the same order as `for_each_child`.
    pub(crate) fn for_each_child_mut(&mut self, mut f: impl FnMut(&mut Hir)) {
        match &mut self.kind {
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

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    f(init);
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
            HirKind::Begin(exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    f(e);
                }
            }
            HirKind::Break { value, .. } => f(value),
            // Call children in EXECUTION order: `lower_call` evaluates the
            // arguments first and the func expression last (both the plain
            // and splice paths — src/lir/lower/control/call.rs), and
            // `compute_order` derives the structural execution order every
            // liveness/region decision compares from this enumeration.
            // Visiting func first releases a binding whose last read sits
            // in func position at its earlier arg-position read — the
            // nil-stamp mistarget pinned by
            // tests/elle/region-call-func-position-reread.lisp.
            HirKind::Call { func, args, .. } => {
                for a in args {
                    f(&mut a.expr);
                }
                f(func);
            }
            HirKind::Assign { value, .. }
            | HirKind::Define { value, .. }
            | HirKind::MakeCell { value } => f(value),
            HirKind::DerefCell { cell } => f(cell),
            HirKind::SetCell { cell, value } => {
                f(cell);
                f(value);
            }
            HirKind::While { cond, body } => {
                f(cond);
                f(body);
            }
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    f(init);
                }
                f(body);
            }
            HirKind::Recur { args } => {
                for a in args {
                    f(a);
                }
            }
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    f(c);
                    f(b);
                }
                if let Some(eb) = else_branch {
                    f(eb);
                }
            }
            HirKind::Emit { value, .. } => f(value),
            HirKind::Match { value, arms } => {
                f(value);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        f(g);
                    }
                    f(body);
                }
            }
            HirKind::Destructure { value, .. } => f(value),
            HirKind::Eval { expr, env } => {
                f(expr);
                f(env);
            }
            // Visit the KEY before the VALUE, matching `lower_parameterize`'s
            // evaluation order (parameter expression first, then its value).
            // The key is a real evaluated sub-expression, so `compute_order`
            // must rank it — skipping it left a binding whose last read is a
            // parameterize key invisible to decref placement, reclaiming its
            // slot before the parameterize read it (the `capture.rs:47`
            // nil-cell panic, tests/elle/parameters.lisp).
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    f(k);
                    f(v);
                }
                f(body);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    f(a);
                }
            }
            HirKind::Return { value } => f(value),
        }
    }

    /// Visit each immediate child HIR node, read-only, in source order. Public
    /// so out-of-crate analysis and tests can walk the tree the same way the
    /// compiler does — consistent with the already-public `Hir::{kind, id}`. The
    /// mutating twin (`for_each_child_mut`) stays crate-private: rewriting the HIR
    /// is an internal concern. The callback's references live as long as `self`,
    /// so a search may return the child it finds.
    pub fn for_each_child<'a>(&'a self, mut f: impl FnMut(&'a Hir)) {
        match &self.kind {
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

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    f(init);
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
            HirKind::Begin(exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    f(e);
                }
            }
            HirKind::Break { value, .. } => f(value),
            // Call children in EXECUTION order (args, then func) — see the
            // `for_each_child_mut` Call arm above; the two must stay
            // identical.
            HirKind::Call { func, args, .. } => {
                for a in args {
                    f(&a.expr);
                }
                f(func);
            }
            HirKind::Assign { value, .. }
            | HirKind::Define { value, .. }
            | HirKind::MakeCell { value } => f(value),
            HirKind::DerefCell { cell } => f(cell),
            HirKind::SetCell { cell, value } => {
                f(cell);
                f(value);
            }
            HirKind::While { cond, body } => {
                f(cond);
                f(body);
            }
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    f(init);
                }
                f(body);
            }
            HirKind::Recur { args } => {
                for a in args {
                    f(a);
                }
            }
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    f(c);
                    f(b);
                }
                if let Some(eb) = else_branch {
                    f(eb);
                }
            }
            HirKind::Emit { value, .. } => f(value),
            HirKind::Match { value, arms } => {
                f(value);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        f(g);
                    }
                    f(body);
                }
            }
            HirKind::Destructure { value, .. } => f(value),
            HirKind::Eval { expr, env } => {
                f(expr);
                f(env);
            }
            // Key before value, matching `lower_parameterize` (and the
            // mutable twin above). See that arm for why the key must be a
            // visited child.
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    f(k);
                    f(v);
                }
                f(body);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    f(a);
                }
            }
            HirKind::Return { value } => f(value),
        }
    }
}
