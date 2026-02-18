//! HIR-based symbol extraction for IDE features
//!
//! Extracts symbol information from analyzed HIR trees to build a
//! SymbolIndex for Language Server Protocol features (hover, completion,
//! go-to-definition, find-references, rename).

use crate::hir::binding::{BindingId, BindingInfo};
use crate::hir::expr::{Hir, HirKind};
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use std::collections::{HashMap, HashSet};

/// Extract symbol index from analyzed HIR
pub fn extract_symbols_from_hir(
    hir: &Hir,
    bindings: &HashMap<BindingId, BindingInfo>,
    symbols: &SymbolTable,
) -> SymbolIndex {
    let mut index = SymbolIndex::new();
    let mut extractor = HirSymbolExtractor::new(bindings);
    extractor.walk(hir, &mut index, symbols);
    extractor.collect_available(symbols, &mut index);
    index
}

struct HirSymbolExtractor<'a> {
    bindings: &'a HashMap<BindingId, BindingInfo>,
    seen: HashSet<BindingId>,
}

impl<'a> HirSymbolExtractor<'a> {
    fn new(bindings: &'a HashMap<BindingId, BindingInfo>) -> Self {
        Self {
            bindings,
            seen: HashSet::new(),
        }
    }

    fn span_to_loc(span: &crate::syntax::Span) -> SourceLoc {
        SourceLoc::from_line_col(span.line as usize, span.col as usize)
    }

    fn record_definition(
        &mut self,
        binding_id: BindingId,
        kind: SymbolKind,
        span: &crate::syntax::Span,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        if self.seen.contains(&binding_id) {
            return;
        }
        self.seen.insert(binding_id);

        if let Some(info) = self.bindings.get(&binding_id) {
            if let Some(name_str) = symbols.name(info.name) {
                let loc = Self::span_to_loc(span);
                let def = SymbolDef::new(info.name, name_str.to_string(), kind)
                    .with_location(loc.clone());
                index.definitions.insert(info.name, def);
                index.symbol_locations.insert(info.name, loc);
            }
        }
    }

    fn record_usage(
        &mut self,
        binding_id: BindingId,
        span: &crate::syntax::Span,
        index: &mut SymbolIndex,
    ) {
        if let Some(info) = self.bindings.get(&binding_id) {
            let loc = Self::span_to_loc(span);
            index.symbol_usages.entry(info.name).or_default().push(loc);
        }
    }

    fn walk(&mut self, hir: &Hir, index: &mut SymbolIndex, symbols: &SymbolTable) {
        match &hir.kind {
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_) => {}

            HirKind::Var(binding_id) => {
                self.record_usage(*binding_id, &hir.span, index);
            }

            HirKind::Define { name, value } => {
                // For global defines, record using SymbolId directly
                if let Some(name_str) = symbols.name(*name) {
                    let loc = Self::span_to_loc(&hir.span);
                    // Determine kind based on value
                    let kind = if matches!(value.kind, HirKind::Lambda { .. }) {
                        SymbolKind::Function
                    } else {
                        SymbolKind::Variable
                    };
                    let def = SymbolDef::new(*name, name_str.to_string(), kind)
                        .with_location(loc.clone());
                    index.definitions.insert(*name, def);
                    index.symbol_locations.insert(*name, loc);
                }
                self.walk(value, index, symbols);
            }

            HirKind::LocalDefine { binding, value } => {
                let kind = if matches!(value.kind, HirKind::Lambda { .. }) {
                    SymbolKind::Function
                } else {
                    SymbolKind::Variable
                };
                self.record_definition(*binding, kind, &hir.span, index, symbols);
                self.walk(value, index, symbols);
            }

            HirKind::Let { bindings, body } => {
                for (binding_id, init) in bindings {
                    self.record_definition(
                        *binding_id,
                        SymbolKind::Variable,
                        &hir.span,
                        index,
                        symbols,
                    );
                    self.walk(init, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::Letrec { bindings, body } => {
                for (binding_id, init) in bindings {
                    let kind = if matches!(init.kind, HirKind::Lambda { .. }) {
                        SymbolKind::Function
                    } else {
                        SymbolKind::Variable
                    };
                    self.record_definition(*binding_id, kind, &hir.span, index, symbols);
                    self.walk(init, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::Lambda { params, body, .. } => {
                for param in params {
                    self.record_definition(*param, SymbolKind::Variable, &hir.span, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk(cond, index, symbols);
                self.walk(then_branch, index, symbols);
                self.walk(else_branch, index, symbols);
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (cond, body) in clauses {
                    self.walk(cond, index, symbols);
                    self.walk(body, index, symbols);
                }
                if let Some(e) = else_branch {
                    self.walk(e, index, symbols);
                }
            }

            HirKind::Begin(exprs)
            | HirKind::Block(exprs)
            | HirKind::And(exprs)
            | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e, index, symbols);
                }
            }

            HirKind::Call { func, args, .. } => {
                self.walk(func, index, symbols);
                for arg in args {
                    self.walk(arg, index, symbols);
                }
            }

            HirKind::Set { value, target } => {
                self.record_usage(*target, &hir.span, index);
                self.walk(value, index, symbols);
            }

            HirKind::While { cond, body } => {
                self.walk(cond, index, symbols);
                self.walk(body, index, symbols);
            }

            HirKind::For { var, iter, body } => {
                self.record_definition(*var, SymbolKind::Variable, &hir.span, index, symbols);
                self.walk(iter, index, symbols);
                self.walk(body, index, symbols);
            }

            HirKind::Match { value, arms } => {
                self.walk(value, index, symbols);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.walk(g, index, symbols);
                    }
                    self.walk(body, index, symbols);
                }
            }

            HirKind::Throw(e) | HirKind::Yield(e) => {
                self.walk(e, index, symbols);
            }

            HirKind::HandlerCase { body, handlers } => {
                self.walk(body, index, symbols);
                for (_, _, handler) in handlers {
                    self.walk(handler, index, symbols);
                }
            }

            HirKind::HandlerBind { handlers, body } => {
                self.walk(body, index, symbols);
                for (_, handler) in handlers {
                    self.walk(handler, index, symbols);
                }
            }

            HirKind::Module { body, .. } => {
                self.walk(body, index, symbols);
            }

            HirKind::Import { .. } | HirKind::ModuleRef { .. } => {}
        }
    }

    fn collect_available(&self, _symbols: &SymbolTable, index: &mut SymbolIndex) {
        for def in index.definitions.values() {
            index
                .available_symbols
                .push((def.name.clone(), def.id, def.kind));
        }
        // Sort for consistent ordering
        index.available_symbols.sort_by(|a, b| a.0.cmp(&b.0));
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
        register_primitives(&mut vm, &mut symbols);
        symbols
    }

    #[test]
    fn test_extract_define_variable() {
        let mut symbols = setup();
        let result = analyze_new("(define x 42)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // Should have one definition
        assert!(!index.definitions.is_empty());
        // Find the 'x' definition
        let x_def = index
            .definitions
            .values()
            .find(|d| d.name == "x")
            .expect("Should have definition for x");
        assert_eq!(x_def.kind, SymbolKind::Variable);
    }

    #[test]
    fn test_extract_define_function() {
        let mut symbols = setup();
        let result = analyze_new("(define add-one (fn (x) (+ x 1)))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // Find the 'add-one' definition
        let add_one_def = index
            .definitions
            .values()
            .find(|d| d.name == "add-one")
            .expect("Should have definition for add-one");
        assert_eq!(add_one_def.kind, SymbolKind::Function);
    }

    #[test]
    fn test_extract_let_bindings() {
        let mut symbols = setup();
        let result = analyze_new("(let ((a 1) (b 2)) (+ a b))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // Should have definitions for a and b
        let has_a = index.definitions.values().any(|d| d.name == "a");
        let has_b = index.definitions.values().any(|d| d.name == "b");
        assert!(has_a, "Should have definition for a");
        assert!(has_b, "Should have definition for b");
    }

    #[test]
    fn test_extract_lambda_params() {
        let mut symbols = setup();
        let result = analyze_new("(fn (x y) (+ x y))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // Should have definitions for x and y parameters
        let has_x = index.definitions.values().any(|d| d.name == "x");
        let has_y = index.definitions.values().any(|d| d.name == "y");
        assert!(has_x, "Should have definition for x");
        assert!(has_y, "Should have definition for y");
    }

    #[test]
    fn test_extract_usages() {
        let mut symbols = setup();
        let result = analyze_new("(let ((x 1)) (+ x x))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // Should have usages for x (used twice in the body)
        let x_sym = symbols.intern("x");
        let usages = index.symbol_usages.get(&x_sym);
        assert!(usages.is_some(), "Should have usages for x");
        // Note: the exact count depends on how the analyzer handles references
    }

    #[test]
    fn test_available_symbols() {
        let mut symbols = setup();
        let result = analyze_new("(begin (define a 1) (define b 2))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();

        let index = extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &symbols);

        // available_symbols should be sorted
        let names: Vec<_> = index.available_symbols.iter().map(|(n, _, _)| n).collect();
        let mut sorted_names = names.clone();
        sorted_names.sort();
        assert_eq!(names, sorted_names, "available_symbols should be sorted");
    }
}
