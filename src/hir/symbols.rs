// audited: 2026-10-23
//! Builds the `SymbolIndex` of an analyzed tree, for the language server's hover, completion and rename.
//!
//! The index also serves go-to-definition and find-references.
//!
//! src/symbols/README.md

use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use std::collections::HashSet;

/// Extract symbol index from analyzed HIR
pub fn extract_symbols_from_hir(
    hir: &Hir,
    symbols: &SymbolTable,
    arena: &crate::hir::BindingArena,
) -> SymbolIndex {
    let mut index = SymbolIndex::new();
    let mut extractor = HirSymbolExtractor::new(arena);
    extractor.walk(hir, &mut index, symbols);
    extractor.collect_available(symbols, &mut index);
    index
}

/// The one arity a single number can state: an exact-arity lambda's declared
/// parameter count. A shape with optionals or a collector answers `None` —
/// `compile/exports` carries the full shape.
fn exact_arity(value: &Hir) -> Option<usize> {
    let lambda = value.kind.as_lambda()?;
    let exact = lambda.rest_param.is_none() && lambda.num_required == lambda.params.len();
    exact.then_some(lambda.params.len())
}

/// The docstring of `value`, when it is a lambda that has one.
fn lambda_doc(value: &Hir) -> Option<String> {
    let doc = value.kind.as_lambda()?.doc.as_ref()?;
    Some(doc.to_string())
}

struct HirSymbolExtractor<'a> {
    seen: HashSet<Binding>,
    arena: &'a crate::hir::BindingArena,
}

impl<'a> HirSymbolExtractor<'a> {
    fn new(arena: &'a crate::hir::BindingArena) -> Self {
        Self {
            seen: HashSet::new(),
            arena,
        }
    }

    /// Translate a span to a source location, preserving the originating file
    /// when the span carries one. The file must be carried through: a location
    /// without it collapses to `<unknown>`, so the LSP emits `file://<unknown>`
    /// URIs and rename filters out every edit (the URI never matches the
    /// document).
    fn span_to_loc(span: &crate::syntax::Span) -> SourceLoc {
        match span.file() {
            Some(file) => SourceLoc::new(file, span.line as usize, span.col as usize),
            None => SourceLoc::from_line_col(span.line as usize, span.col as usize),
        }
    }

    fn record_definition(
        &mut self,
        binding: Binding,
        kind: SymbolKind,
        span: &crate::syntax::Span,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        // Synthetic bindings (file-letrec statement wrappers, signal/destructure
        // gensyms) have no source-level identity the user wrote — keep them out
        // of the index entirely so they never surface as fake symbols.
        if self.arena.get(binding).is_synthetic {
            return;
        }
        if self.seen.contains(&binding) {
            return;
        }
        self.seen.insert(binding);

        let sym = self.arena.get(binding).name;
        if let Some(name_str) = symbols.name(sym) {
            let loc = Self::span_to_loc(span);
            let def = SymbolDef::new(sym, name_str.to_string(), kind).with_location(loc.clone());
            // Overwrites any placeholder entry a forward-referencing usage may
            // have inserted (see `record_usage`).
            index.definitions.insert(binding.def_id(), def);
            index.symbol_locations.insert(binding.def_id(), loc);
        }
    }

    fn record_usage(
        &mut self,
        binding: Binding,
        span: &crate::syntax::Span,
        index: &mut SymbolIndex,
        symbols: &SymbolTable,
    ) {
        if self.arena.get(binding).is_synthetic {
            return;
        }
        let id = binding.def_id();
        // Ensure a name is always resolvable for this binding, even when it is
        // only ever *used* and never defined in this file (primitives, globals).
        // Such placeholder entries carry no location and kind `Builtin`; a real
        // definition later overwrites them via `record_definition`.
        if let std::collections::hash_map::Entry::Vacant(entry) = index.definitions.entry(id) {
            let sym = self.arena.get(binding).name;
            if let Some(name_str) = symbols.name(sym) {
                entry.insert(SymbolDef::new(
                    sym,
                    name_str.to_string(),
                    SymbolKind::Builtin,
                ));
            }
        }
        let loc = Self::span_to_loc(span);
        index.symbol_usages.entry(id).or_default().push(loc);
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
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_) => {}

            HirKind::Var(binding) => {
                self.record_usage(*binding, &hir.span, index, symbols);
            }

            HirKind::Define { binding, value } => {
                let kind = if matches!(value.kind, HirKind::Lambda(_)) {
                    SymbolKind::Function
                } else {
                    SymbolKind::Variable
                };
                let doc_string = lambda_doc(value);
                self.record_definition(*binding, kind, &hir.span, index, symbols);
                if let Some(def) = index.definitions.get_mut(&binding.def_id()) {
                    if let Some(doc_str) = doc_string {
                        def.documentation = Some(doc_str);
                    }
                    def.arity = exact_arity(value);
                }
                self.walk(value, index, symbols);
            }

            HirKind::Destructure { pattern, value, .. } => {
                // Record all bindings in the pattern
                for binding in pattern.bindings().bindings {
                    self.record_definition(
                        binding,
                        SymbolKind::Variable,
                        &hir.span,
                        index,
                        symbols,
                    );
                }
                self.walk(value, index, symbols);
            }

            HirKind::Let { bindings, body } => {
                for (binding_id, init) in bindings {
                    self.record_definition(
                        *binding_id,
                        SymbolKind::Variable,
                        &init.span,
                        index,
                        symbols,
                    );
                    self.walk(init, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::Letrec { bindings, body } => {
                for (binding_id, init) in bindings {
                    let kind = if matches!(init.kind, HirKind::Lambda(_)) {
                        SymbolKind::Function
                    } else {
                        SymbolKind::Variable
                    };
                    self.record_definition(*binding_id, kind, &init.span, index, symbols);
                    if let Some(def) = index.definitions.get_mut(&binding_id.def_id()) {
                        if let Some(doc_str) = lambda_doc(init) {
                            def.documentation = Some(doc_str);
                        }
                        def.arity = exact_arity(init);
                    }
                    self.walk(init, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::Lambda(lambda) => {
                for param in &lambda.params {
                    self.record_definition(*param, SymbolKind::Variable, &hir.span, index, symbols);
                }
                self.walk(&lambda.body, index, symbols);
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

            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e, index, symbols);
                }
            }

            HirKind::Block { body, .. } => {
                for e in body {
                    self.walk(e, index, symbols);
                }
            }

            HirKind::Break { value, .. } => {
                self.walk(value, index, symbols);
            }

            HirKind::Call { func, args, .. } => {
                self.walk(func, index, symbols);
                for arg in args {
                    self.walk(&arg.expr, index, symbols);
                }
            }

            HirKind::Assign { target, value } => {
                self.record_usage(*target, &hir.span, index, symbols);
                self.walk(value, index, symbols);
            }

            HirKind::While { cond, body } => {
                self.walk(cond, index, symbols);
                self.walk(body, index, symbols);
            }

            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    self.walk(init, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::Recur { args } => {
                for arg in args {
                    self.walk(arg, index, symbols);
                }
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

            HirKind::Emit { value: e, .. } => {
                self.walk(e, index, symbols);
            }

            HirKind::Return { value } => {
                self.walk(value, index, symbols);
            }

            HirKind::Eval { expr, env } => {
                self.walk(expr, index, symbols);
                self.walk(env, index, symbols);
            }

            HirKind::Parameterize { bindings, body } => {
                for (param, value) in bindings {
                    self.walk(param, index, symbols);
                    self.walk(value, index, symbols);
                }
                self.walk(body, index, symbols);
            }

            HirKind::MakeCell { value } => {
                self.walk(value, index, symbols);
            }
            HirKind::DerefCell { cell } => {
                self.walk(cell, index, symbols);
            }
            HirKind::SetCell { cell, value } => {
                self.walk(cell, index, symbols);
                self.walk(value, index, symbols);
            }

            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    self.walk(a, index, symbols);
                }
            }

            HirKind::Error => {}
        }
    }

    fn collect_available(&self, _symbols: &SymbolTable, index: &mut SymbolIndex) {
        // Only real in-file definitions are completion candidates. Usage-only
        // placeholder entries (primitives) carry no location and are excluded;
        // completion sources those from the VM's docs map instead.
        let mut avail: Vec<(String, crate::symbols::DefId, SymbolKind)> = index
            .definitions
            .iter()
            .filter(|(_, def)| def.location.is_some())
            .map(|(id, def)| (def.name.clone(), *id, def.kind))
            .collect();
        // Sort for consistent ordering
        avail.sort_by(|a, b| a.0.cmp(&b.0));
        index.available_symbols = avail;
    }
}

#[cfg(test)]
mod tests;
