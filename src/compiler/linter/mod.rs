//! Linting and diagnostics integrated into the compiler
//!
//! Provides comprehensive static analysis for Elle code including:
//! - Naming conventions
//! - Arity validation
//! - Unused variable detection
//! - Pattern matching validation

pub mod diagnostics;
pub mod rules;

use super::ast::{Expr, ExprWithLoc};
use crate::reader::SourceLoc;
use diagnostics::Diagnostic;

/// A linter that operates on compiled Expr AST
pub struct Linter {
    diagnostics: Vec<Diagnostic>,
}

impl Linter {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Lint a single expression with location info
    pub fn lint_expr(&mut self, expr: &ExprWithLoc, symbol_table: &crate::SymbolTable) {
        self.check_expr(&expr.expr, &expr.loc, symbol_table);
    }

    /// Lint multiple expressions
    pub fn lint_exprs(&mut self, exprs: &[ExprWithLoc], symbol_table: &crate::SymbolTable) {
        for expr in exprs {
            self.lint_expr(expr, symbol_table);
        }
    }

    fn check_expr(
        &mut self,
        expr: &Expr,
        loc: &Option<SourceLoc>,
        symbol_table: &crate::SymbolTable,
    ) {
        match expr {
            Expr::Literal(_) => {
                // No linting needed for literals
            }

            Expr::Var(_, _, _) => {
                // Could add undefined variable checks here
            }

            Expr::GlobalVar(_) => {
                // Global variables are assumed to exist
            }

            Expr::If { cond, then, else_ } => {
                self.check_expr(cond, loc, symbol_table);
                self.check_expr(then, loc, symbol_table);
                self.check_expr(else_, loc, symbol_table);
            }

            Expr::Cond { clauses, else_body } => {
                for (cond, body) in clauses {
                    self.check_expr(cond, loc, symbol_table);
                    self.check_expr(body, loc, symbol_table);
                }
                if let Some(else_body) = else_body {
                    self.check_expr(else_body, loc, symbol_table);
                }
            }

            Expr::Begin(exprs) => {
                for e in exprs {
                    self.check_expr(e, loc, symbol_table);
                }
            }

            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr(e, loc, symbol_table);
                }
            }

            Expr::Call { func, args, .. } => {
                self.check_expr(func, loc, symbol_table);
                for arg in args {
                    self.check_expr(arg, loc, symbol_table);
                }
                // Check arity if we can determine the function
                if let Expr::GlobalVar(sym) = &**func {
                    rules::check_call_arity(
                        *sym,
                        args.len(),
                        loc,
                        symbol_table,
                        &mut self.diagnostics,
                    );
                }
            }

            Expr::Lambda { body, .. } => {
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Let { bindings, body } => {
                for (_, init) in bindings {
                    self.check_expr(init, loc, symbol_table);
                }
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    self.check_expr(init, loc, symbol_table);
                }
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Set { value, .. } => {
                self.check_expr(value, loc, symbol_table);
            }

            Expr::Define { name, value } => {
                self.check_expr(value, loc, symbol_table);
                // Check naming conventions on the defined symbol
                if let Some(sym_name) = symbol_table.name(*name) {
                    rules::check_naming_convention(sym_name, loc, &mut self.diagnostics);
                }
            }

            Expr::While { cond, body } => {
                self.check_expr(cond, loc, symbol_table);
                self.check_expr(body, loc, symbol_table);
            }

            Expr::For { iter, body, .. } => {
                self.check_expr(iter, loc, symbol_table);
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Match {
                value,
                patterns: _,
                default,
            } => {
                self.check_expr(value, loc, symbol_table);
                if let Some(default) = default {
                    self.check_expr(default, loc, symbol_table);
                }
            }

            Expr::Try {
                body,
                catch,
                finally,
            } => {
                self.check_expr(body, loc, symbol_table);
                if let Some((_, handler)) = catch {
                    self.check_expr(handler, loc, symbol_table);
                }
                if let Some(finally) = finally {
                    self.check_expr(finally, loc, symbol_table);
                }
            }

            Expr::Throw { value } => {
                self.check_expr(value, loc, symbol_table);
            }

            Expr::HandlerCase { body, handlers } => {
                self.check_expr(body, loc, symbol_table);
                for (_exc_id, _var, handler_expr) in handlers {
                    self.check_expr(handler_expr, loc, symbol_table);
                }
            }

            Expr::HandlerBind { handlers, body } => {
                self.check_expr(body, loc, symbol_table);
                for (_exc_id, handler_fn) in handlers {
                    self.check_expr(handler_fn, loc, symbol_table);
                }
            }

            Expr::Quote(_) => {
                // Quoted expressions are not evaluated
            }

            Expr::Quasiquote(expr) | Expr::Unquote(expr) => {
                self.check_expr(expr, loc, symbol_table);
            }

            Expr::DefMacro { body, .. } => {
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Module { body, .. } => {
                self.check_expr(body, loc, symbol_table);
            }

            Expr::Import { .. } => {
                // No linting needed for imports
            }

            Expr::ModuleRef { .. } => {
                // No linting needed for module refs
            }

            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
                for e in exprs {
                    self.check_expr(e, loc, symbol_table);
                }
            }

            Expr::ScopeVar(_, _) | Expr::ScopeEntry(_) | Expr::ScopeExit => {
                // No linting needed for scope markers
            }
        }
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
            .any(|d| d.severity == diagnostics::Severity::Error)
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == diagnostics::Severity::Warning)
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linter_creation() {
        let linter = Linter::new();
        assert_eq!(linter.diagnostics().len(), 0);
        assert!(!linter.has_errors());
        assert!(!linter.has_warnings());
    }
}
