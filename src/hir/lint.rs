//! HIR-based linter
//!
//! Walks HIR trees and produces diagnostics. Uses the same rules as the
//! legacy Expr-based linter but operates on the new pipeline's HIR.

use crate::hir::arena::BindingArena;
use crate::hir::expr::{Hir, HirKind};
use crate::hir::pattern::is_exhaustive_match;
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::lint::rules;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;

/// HIR-based linter
pub struct HirLinter {
    diagnostics: Vec<Diagnostic>,
}

impl HirLinter {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Lint a single HIR expression
    pub fn lint(&mut self, hir: &Hir, symbols: &SymbolTable, arena: &BindingArena) {
        self.check(hir, symbols, arena);
    }

    /// Get all diagnostics
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Get mutable diagnostics
    pub fn diagnostics_mut(&mut self) -> &mut Vec<Diagnostic> {
        &mut self.diagnostics
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
                for (_, init) in bindings {
                    self.check(init, symbols, arena);
                }
                self.check(body, symbols, arena);
            }

            HirKind::Letrec { bindings, body } => {
                for (binding, init) in bindings {
                    // Check naming convention on file-level def/var bindings.
                    // Skip gensyms (__file_expr_N) and primitive bindings
                    // (identified by their initializer being a quoted NativeFn).
                    let is_primitive = matches!(&init.kind, HirKind::Quote(v) if v.is_native_fn());
                    if !is_primitive {
                        if let Some(sym_name) = symbols.name(arena.get(*binding).name) {
                            if !sym_name.starts_with("__") {
                                let binding_loc = Self::span_to_loc(&init.span);
                                rules::check_naming_convention(
                                    sym_name,
                                    &binding_loc,
                                    &mut self.diagnostics,
                                );
                            }
                        }
                    }
                    self.check(init, symbols, arena);
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

            HirKind::Call { func, args, .. } => {
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
            }

            HirKind::Assign { value, .. } => {
                self.check(value, symbols, arena);
            }

            HirKind::Define { binding, value } => {
                // Check naming convention
                if let Some(sym_name) = symbols.name(arena.get(*binding).name) {
                    rules::check_naming_convention(sym_name, &loc, &mut self.diagnostics);
                }
                self.check(value, symbols, arena);
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

                // Check for non-exhaustive match
                if !arms.is_empty() && !is_exhaustive_match(arms) {
                    self.diagnostics.push(Diagnostic::new(
                        Severity::Warning,
                        "W003",
                        "non-exhaustive-match",
                        "match expression may not cover all cases; consider adding a wildcard (_) or variable pattern as the last arm",
                        loc.clone(),
                    ));
                }
            }

            HirKind::Emit { value: expr, .. } => {
                self.check(expr, symbols, arena);
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

            HirKind::Quote(_) => {}

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
mod tests {
    use super::*;
    use crate::pipeline::analyze;
    use crate::primitives::register_primitives;
    use crate::vm::VM;

    fn setup() -> (SymbolTable, VM) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);
        (symbols, vm)
    }

    #[test]
    fn test_hir_linter_creation() {
        let linter = HirLinter::new();
        assert_eq!(linter.diagnostics().len(), 0);
        assert!(!linter.has_errors());
        assert!(!linter.has_warnings());
    }

    #[test]
    fn test_hir_linter_naming_convention() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("(var camelCase 42)", &mut symbols, &mut vm, "<test>");
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new();
        linter.lint(&analysis.hir, &symbols, &analysis.arena);

        assert!(linter.has_warnings());
        assert!(linter
            .diagnostics()
            .iter()
            .any(|d| d.rule == "naming-kebab-case"));
    }

    #[test]
    fn test_hir_linter_valid_naming() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("(var valid-name 42)", &mut symbols, &mut vm, "<test>");
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new();
        linter.lint(&analysis.hir, &symbols, &analysis.arena);

        // Should have no naming warnings
        assert!(!linter
            .diagnostics()
            .iter()
            .any(|d| d.rule == "naming-kebab-case"));
    }

    #[test]
    fn test_hir_linter_arity_check() {
        let (mut symbols, mut vm) = setup();
        // cons expects 2 arguments — the analyzer catches this as a hard error
        let result = analyze("(pair 1)", &mut symbols, &mut vm, "<test>");
        match result {
            Err(ref msg) => assert!(
                msg.contains("arity error"),
                "expected arity error, got: {msg}"
            ),
            Ok(_) => panic!("expected arity error for (pair 1)"),
        }
    }

    #[test]
    fn test_hir_linter_nested_expressions() {
        let (mut symbols, mut vm) = setup();
        let result = analyze(
            "(let [camelCase 1] (if true camelCase 0))",
            &mut symbols,
            &mut vm,
            "<test>",
        );
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new();
        linter.lint(&analysis.hir, &symbols, &analysis.arena);

        // Let bindings don't trigger naming convention checks (only define does)
        // This is consistent with the legacy linter behavior
        assert!(!linter.has_warnings());
    }
}
