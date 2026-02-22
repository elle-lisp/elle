//! HIR-based linter
//!
//! Walks HIR trees and produces diagnostics. Uses the same rules as the
//! legacy Expr-based linter but operates on the new pipeline's HIR.

use crate::hir::binding::{BindingId, BindingInfo, BindingKind};
use crate::hir::expr::{Hir, HirKind};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::lint::rules;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use std::collections::HashMap;

/// HIR-based linter
pub struct HirLinter {
    diagnostics: Vec<Diagnostic>,
    bindings: HashMap<BindingId, BindingInfo>,
}

impl HirLinter {
    pub fn new(bindings: HashMap<BindingId, BindingInfo>) -> Self {
        Self {
            diagnostics: Vec::new(),
            bindings,
        }
    }

    /// Lint a single HIR expression
    pub fn lint(&mut self, hir: &Hir, symbols: &SymbolTable) {
        self.check(hir, symbols);
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

    fn check(&mut self, hir: &Hir, symbols: &SymbolTable) {
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

            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    self.check(init, symbols);
                }
                self.check(body, symbols);
            }

            HirKind::Lambda { body, .. } => {
                self.check(body, symbols);
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check(cond, symbols);
                self.check(then_branch, symbols);
                self.check(else_branch, symbols);
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (cond, body) in clauses {
                    self.check(cond, symbols);
                    self.check(body, symbols);
                }
                if let Some(else_body) = else_branch {
                    self.check(else_body, symbols);
                }
            }

            HirKind::Begin(exprs) | HirKind::Block(exprs) => {
                for e in exprs {
                    self.check(e, symbols);
                }
            }

            HirKind::Call { func, args, .. } => {
                self.check(func, symbols);
                for arg in args {
                    self.check(arg, symbols);
                }
                // Check arity if calling a known global
                if let HirKind::Var(binding_id) = &func.kind {
                    if let Some(info) = self.bindings.get(binding_id) {
                        if let BindingKind::Global = info.kind {
                            rules::check_call_arity(
                                info.name,
                                args.len(),
                                &loc,
                                symbols,
                                &mut self.diagnostics,
                            );
                        }
                    }
                }
            }

            HirKind::Set { value, .. } => {
                self.check(value, symbols);
            }

            HirKind::Define { name, value } => {
                // Check naming convention
                if let Some(sym_name) = symbols.name(*name) {
                    rules::check_naming_convention(sym_name, &loc, &mut self.diagnostics);
                }
                self.check(value, symbols);
            }

            HirKind::LocalDefine { value, .. } => {
                self.check(value, symbols);
            }

            HirKind::While { cond, body } => {
                self.check(cond, symbols);
                self.check(body, symbols);
            }

            HirKind::For { iter, body, .. } => {
                self.check(iter, symbols);
                self.check(body, symbols);
            }

            HirKind::Match { value, arms } => {
                self.check(value, symbols);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.check(g, symbols);
                    }
                    self.check(body, symbols);
                }
            }

            HirKind::Yield(expr) => {
                self.check(expr, symbols);
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.check(e, symbols);
                }
            }

            HirKind::Quote(_) => {}

            HirKind::Module { body, .. } => {
                self.check(body, symbols);
            }

            HirKind::Import { .. } | HirKind::ModuleRef { .. } => {}
        }
    }
}

impl Default for HirLinter {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::analyze_new;
    use crate::primitives::register_primitives;
    use crate::vm::VM;

    fn setup() -> SymbolTable {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _effects = register_primitives(&mut vm, &mut symbols);
        symbols
    }

    #[test]
    fn test_hir_linter_creation() {
        let linter = HirLinter::new(HashMap::new());
        assert_eq!(linter.diagnostics().len(), 0);
        assert!(!linter.has_errors());
        assert!(!linter.has_warnings());
    }

    #[test]
    fn test_hir_linter_naming_convention() {
        let mut symbols = setup();
        let result = analyze_new("(define camelCase 42)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new(analysis.bindings);
        linter.lint(&analysis.hir, &symbols);

        assert!(linter.has_warnings());
        assert!(linter
            .diagnostics()
            .iter()
            .any(|d| d.rule == "naming-kebab-case"));
    }

    #[test]
    fn test_hir_linter_valid_naming() {
        let mut symbols = setup();
        let result = analyze_new("(define valid-name 42)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new(analysis.bindings);
        linter.lint(&analysis.hir, &symbols);

        // Should have no naming warnings
        assert!(!linter
            .diagnostics()
            .iter()
            .any(|d| d.rule == "naming-kebab-case"));
    }

    #[test]
    fn test_hir_linter_arity_check() {
        let mut symbols = setup();
        // cons expects 2 arguments
        let result = analyze_new("(cons 1)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new(analysis.bindings);
        linter.lint(&analysis.hir, &symbols);

        assert!(linter.has_warnings());
        assert!(linter
            .diagnostics()
            .iter()
            .any(|d| d.rule == "arity-mismatch"));
    }

    #[test]
    fn test_hir_linter_nested_expressions() {
        let mut symbols = setup();
        let result = analyze_new("(let ((camelCase 1)) (if #t camelCase 0))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let mut linter = HirLinter::new(analysis.bindings);
        linter.lint(&analysis.hir, &symbols);

        // Let bindings don't trigger naming convention checks (only define does)
        // This is consistent with the legacy linter behavior
        assert!(!linter.has_warnings());
    }
}
