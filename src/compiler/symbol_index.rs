//! Symbol extraction for the legacy Expr-based pipeline
//!
//! Extracts symbol information from compiled Expr trees to enable
//! Language Server Protocol features. The data types live in
//! `crate::symbols`; this module provides the extraction logic
//! for the old pipeline.

use super::ast::{Expr, ExprWithLoc};
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use crate::value::SymbolId;
use std::collections::HashSet;

// Re-export the types for backward compatibility
pub use crate::symbols::get_primitive_documentation;

/// Extract symbol index from compiled expressions
pub fn extract_symbols(exprs: &[ExprWithLoc], symbols: &SymbolTable) -> SymbolIndex {
    let mut index = SymbolIndex::new();
    let mut extractor = SymbolExtractor::new();

    for expr in exprs {
        extractor.walk_expr_with_loc(expr, &mut index, symbols);
    }

    // Add all available symbols to the index
    extractor.collect_available_symbols(&mut index, symbols);

    index
}

/// Helper to extract symbols from Expr tree
struct SymbolExtractor {
    seen_definitions: HashSet<SymbolId>,
}

impl SymbolExtractor {
    fn new() -> Self {
        Self {
            seen_definitions: HashSet::new(),
        }
    }

    fn walk_expr_with_loc(
        &mut self,
        expr_with_loc: &ExprWithLoc,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        self.walk_expr(&expr_with_loc.expr, &expr_with_loc.loc, index, symbols);
    }

    fn walk_expr(
        &mut self,
        expr: &Expr,
        loc: &Option<SourceLoc>,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        match expr {
            Expr::Literal(_) => {}

            Expr::Var(var_ref) => {
                if let Some(source_loc) = loc {
                    if let crate::binding::VarRef::Global { sym } = var_ref {
                        index
                            .symbol_usages
                            .entry(*sym)
                            .or_default()
                            .push(source_loc.clone());
                    }
                }
            }

            Expr::Define { name, value } => {
                // Record the definition
                if let Some(source_loc) = loc {
                    index.symbol_locations.insert(*name, source_loc.clone());
                }

                if !self.seen_definitions.contains(name) {
                    self.seen_definitions.insert(*name);

                    if let Some(name_str) = symbols.name(*name) {
                        let def = SymbolDef::new(*name, name_str.to_string(), SymbolKind::Variable)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );

                        index.definitions.insert(*name, def);
                    }
                }

                self.walk_expr(value, loc, index, symbols);
            }

            Expr::Lambda { body, params, .. } => {
                // Record parameters as variables
                for param in params {
                    if let Some(param_str) = symbols.name(*param) {
                        let def =
                            SymbolDef::new(*param, param_str.to_string(), SymbolKind::Variable)
                                .with_location(
                                    loc.as_ref()
                                        .cloned()
                                        .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                                );
                        index.definitions.insert(*param, def);
                    }
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Let { bindings, body } => {
                // Record let bindings as variables
                for (var, init) in bindings {
                    if let Some(var_str) = symbols.name(*var) {
                        let def = SymbolDef::new(*var, var_str.to_string(), SymbolKind::Variable)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );
                        index.definitions.insert(*var, def);
                    }
                    self.walk_expr(init, loc, index, symbols);
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Letrec { bindings, body } => {
                // Record letrec bindings as functions
                for (var, init) in bindings {
                    if let Some(var_str) = symbols.name(*var) {
                        let def = SymbolDef::new(*var, var_str.to_string(), SymbolKind::Function)
                            .with_location(
                                loc.as_ref()
                                    .cloned()
                                    .unwrap_or_else(|| SourceLoc::from_line_col(0, 0)),
                            );
                        index.definitions.insert(*var, def);
                    }
                    self.walk_expr(init, loc, index, symbols);
                }
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::If { cond, then, else_ } => {
                self.walk_expr(cond, loc, index, symbols);
                self.walk_expr(then, loc, index, symbols);
                self.walk_expr(else_, loc, index, symbols);
            }

            Expr::Cond { clauses, else_body } => {
                for (cond, body) in clauses {
                    self.walk_expr(cond, loc, index, symbols);
                    self.walk_expr(body, loc, index, symbols);
                }
                if let Some(else_body) = else_body {
                    self.walk_expr(else_body, loc, index, symbols);
                }
            }

            Expr::Begin(exprs) | Expr::Block(exprs) => {
                for e in exprs {
                    self.walk_expr(e, loc, index, symbols);
                }
            }

            Expr::Call { func, args, .. } => {
                self.walk_expr(func, loc, index, symbols);
                for arg in args {
                    self.walk_expr(arg, loc, index, symbols);
                }
            }

            Expr::While { cond, body } => {
                self.walk_expr(cond, loc, index, symbols);
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::For { iter, body, .. } => {
                self.walk_expr(iter, loc, index, symbols);
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Match {
                value,
                patterns: _,
                default,
            } => {
                self.walk_expr(value, loc, index, symbols);
                if let Some(default) = default {
                    self.walk_expr(default, loc, index, symbols);
                }
            }

            Expr::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_expr(body, loc, index, symbols);
                if let Some((_, handler)) = catch {
                    self.walk_expr(handler, loc, index, symbols);
                }
                if let Some(finally) = finally {
                    self.walk_expr(finally, loc, index, symbols);
                }
            }

            Expr::Throw { value } => {
                self.walk_expr(value, loc, index, symbols);
            }

            Expr::HandlerCase { body, handlers } => {
                self.walk_expr(body, loc, index, symbols);
                for (_exc_id, _var, handler_expr) in handlers {
                    self.walk_expr(handler_expr, loc, index, symbols);
                }
            }

            Expr::HandlerBind { handlers, body } => {
                self.walk_expr(body, loc, index, symbols);
                for (_exc_id, handler_fn) in handlers {
                    self.walk_expr(handler_fn, loc, index, symbols);
                }
            }

            Expr::Quote(_) | Expr::Quasiquote(_) | Expr::Unquote(_) => {
                // Don't walk quoted expressions
            }

            Expr::Set { value, .. } => {
                self.walk_expr(value, loc, index, symbols);
            }

            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
                for e in exprs {
                    self.walk_expr(e, loc, index, symbols);
                }
            }

            Expr::DefMacro { body, .. } => {
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Module { body, .. } => {
                self.walk_expr(body, loc, index, symbols);
            }

            Expr::Import { .. } | Expr::ModuleRef { .. } => {
                // Module references are handled elsewhere
            }

            Expr::Yield(expr) => {
                self.walk_expr(expr, loc, index, symbols);
            }
        }
    }

    fn collect_available_symbols(&self, index: &mut SymbolIndex, _symbols: &SymbolTable) {
        // Collect builtins and defined symbols
        for (sym_id, def) in &index.definitions {
            index
                .available_symbols
                .push((def.name.clone(), *sym_id, def.kind));
        }

        // Sort for consistent ordering
        index.available_symbols.sort_by(|a, b| a.0.cmp(&b.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_index_creation() {
        let index = SymbolIndex::new();
        assert_eq!(index.definitions.len(), 0);
        assert_eq!(index.available_symbols.len(), 0);
    }
}
