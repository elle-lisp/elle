//! HIR-based linter
//!
//! Walks HIR trees and produces diagnostics. Uses the same rules as the
//! legacy Expr-based linter but operates on the new pipeline's HIR.

use std::collections::HashSet;

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::expr::{Hir, HirKind};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::lint::rules;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;

/// HIR-based linter
pub struct HirLinter {
    diagnostics: Vec<Diagnostic>,
    /// The nearest enclosing named function while walking the tree. Stamped onto
    /// each diagnostic (`Diagnostic::function`) so per-function consumers can
    /// attribute a finding exactly. `None` at module/top level.
    current_fn: Option<SymbolId>,
    /// The binding `current_fn` names. Identity, where `current_fn` is only a
    /// spelling: a self-call is a call whose function position resolves to this
    /// binding, which no name comparison can decide once a name is shadowed.
    current_fn_binding: Option<Binding>,
    /// Every binding with at least one use, over the whole tree handed to
    /// [`lint`](Self::lint). Read by the unused-binding rule. Collected up front
    /// because a use may appear anywhere — including textually before the
    /// binding, in a `letrec` — so the single-pass walk cannot decide it.
    used: HashSet<Binding>,
}

impl HirLinter {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            current_fn: None,
            current_fn_binding: None,
            used: HashSet::new(),
        }
    }

    /// Lint a whole HIR tree.
    ///
    /// `hir` must be a complete compilation unit: the def-use pass below runs
    /// over it once, and a binding whose only use sits outside the subtree would
    /// read as unused. Analysis marks tail calls (`pipeline::analyze`), so the
    /// `is_tail` flags this reads are the same ones lowering acts on.
    pub fn lint(&mut self, hir: &Hir, symbols: &SymbolTable, arena: &BindingArena) {
        let mut defuse = DefUseBuilder::new();
        defuse.walk(hir);
        self.used = defuse.uses.into_keys().collect();
        self.check(hir, symbols, arena);
    }

    /// Get all diagnostics
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning)
    }

    /// Convert Span to SourceLoc for rules
    fn span_to_loc(span: &crate::syntax::Span) -> Option<SourceLoc> {
        Some(SourceLoc::from_line_col(
            span.line as usize,
            span.col as usize,
        ))
    }

    /// Lint a `def`/`let`/`letrec` binding introduction, then descend into its
    /// initializer — entering `binding`'s function scope first when the
    /// initializer is its lambda body, so findings inside attribute to it. This
    /// is the single site that maintains `current_fn`; loop/match/pattern
    /// bindings never reach it, which is why their bindings are not flagged.
    fn check_binding_site(
        &mut self,
        binding: crate::hir::Binding,
        init: &Hir,
        loc: &Option<SourceLoc>,
        symbols: &SymbolTable,
        arena: &BindingArena,
    ) {
        let fname = self.current_fn.and_then(|s| symbols.name(s));
        rules::check_mutable_never_assigned(
            binding,
            arena,
            loc,
            symbols,
            fname,
            &mut self.diagnostics,
        );
        rules::check_unused_binding(
            binding,
            arena,
            &self.used,
            loc,
            symbols,
            fname,
            &mut self.diagnostics,
        );
        let prev = (self.current_fn, self.current_fn_binding);
        if !arena.get(binding).is_synthetic && matches!(init.kind, HirKind::Lambda { .. }) {
            self.current_fn = Some(arena.get(binding).name);
            self.current_fn_binding = Some(binding);
        }
        self.check(init, symbols, arena);
        (self.current_fn, self.current_fn_binding) = prev;
    }

    fn check(&mut self, hir: &Hir, symbols: &SymbolTable, arena: &BindingArena) {
        let loc = Self::span_to_loc(&hir.span);

        match &hir.kind {
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_) => {}

            HirKind::Var(_) => {}

            HirKind::Let { bindings, body } => {
                for (binding, init) in bindings {
                    let bloc = Self::span_to_loc(&init.span);
                    self.check_binding_site(*binding, init, &bloc, symbols, arena);
                }
                self.check(body, symbols, arena);
            }

            HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    let bloc = Self::span_to_loc(&init.span);
                    self.check_binding_site(*binding, init, &bloc, symbols, arena);
                }
                self.check(body, symbols, arena);
            }

            HirKind::Lambda { body, .. } => {
                self.check(body, symbols, arena);
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check(cond, symbols, arena);
                self.check(then_branch, symbols, arena);
                self.check(else_branch, symbols, arena);
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (cond, body) in clauses {
                    self.check(cond, symbols, arena);
                    self.check(body, symbols, arena);
                }
                if let Some(else_body) = else_branch {
                    self.check(else_body, symbols, arena);
                }
            }

            HirKind::Begin(exprs) => {
                for e in exprs {
                    self.check(e, symbols, arena);
                }
            }

            HirKind::Block { body, .. } => {
                for e in body {
                    self.check(e, symbols, arena);
                }
            }

            HirKind::Break { value, .. } => {
                self.check(value, symbols, arena);
            }

            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                self.check(func, symbols, arena);
                for arg in args {
                    self.check(&arg.expr, symbols, arena);
                }
                // Check arity if calling a known primitive (skip if any spliced args)
                let has_splice = args.iter().any(|a| a.spliced);
                if !has_splice {
                    if let HirKind::Var(binding) = &func.kind {
                        rules::check_call_arity(
                            arena.get(*binding).name,
                            args.len(),
                            &loc,
                            symbols,
                            &mut self.diagnostics,
                        );
                    }
                }
                if let (Some(enclosing), HirKind::Var(callee)) =
                    (self.current_fn_binding, &func.kind)
                {
                    rules::check_non_tail_self_recursion(
                        enclosing,
                        *callee,
                        *is_tail,
                        arena,
                        &loc,
                        symbols,
                        &mut self.diagnostics,
                    );
                }
            }

            HirKind::Assign { value, .. } => {
                self.check(value, symbols, arena);
            }

            HirKind::Define { binding, value } => {
                self.check_binding_site(*binding, value, &loc, symbols, arena);
            }

            HirKind::Destructure { value, .. } => {
                self.check(value, symbols, arena);
            }

            HirKind::While { cond, body } => {
                self.check(cond, symbols, arena);
                self.check(body, symbols, arena);
            }

            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    self.check(init, symbols, arena);
                }
                self.check(body, symbols, arena);
            }

            HirKind::Recur { args } => {
                for arg in args {
                    self.check(arg, symbols, arena);
                }
            }

            HirKind::Match { value, arms } => {
                self.check(value, symbols, arena);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.check(g, symbols, arena);
                    }
                    self.check(body, symbols, arena);
                }
            }

            HirKind::Emit { value: expr, .. } => {
                self.check(expr, symbols, arena);
            }

            HirKind::Return { value } => {
                self.check(value, symbols, arena);
            }

            HirKind::Eval { expr, env } => {
                self.check(expr, symbols, arena);
                self.check(env, symbols, arena);
            }

            HirKind::Parameterize { bindings, body } => {
                for (param, value) in bindings {
                    self.check(param, symbols, arena);
                    self.check(value, symbols, arena);
                }
                self.check(body, symbols, arena);
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.check(e, symbols, arena);
                }
            }

            HirKind::MakeCell { value } => {
                self.check(value, symbols, arena);
            }
            HirKind::DerefCell { cell } => {
                self.check(cell, symbols, arena);
            }
            HirKind::SetCell { cell, value } => {
                self.check(cell, symbols, arena);
                self.check(value, symbols, arena);
            }

            HirKind::Quote(_) | HirKind::QuoteConst(_) => {}

            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    self.check(a, symbols, arena);
                }
            }

            HirKind::Error => {}
        }
    }
}

impl Default for HirLinter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
