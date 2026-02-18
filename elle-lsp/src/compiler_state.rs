//! Resident compiler state for LSP server
//!
//! Manages compilation state for open documents and provides
//! symbol index for IDE features.

use elle::hir::{extract_symbols_from_hir, HirLinter};
use elle::lint::diagnostics::{Diagnostic, Severity};
use elle::symbol::SymbolTable;
use elle::symbols::SymbolIndex;
use elle::{analyze_all_new, init_stdlib, register_primitives, VM};
use std::collections::HashMap;

/// Document state: source + diagnostics + symbol index
pub struct DocumentState {
    pub uri: String,
    pub source_text: String,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentState {
    fn new(uri: String) -> Self {
        Self {
            uri,
            source_text: String::new(),
            symbol_index: SymbolIndex::new(),
            diagnostics: Vec::new(),
        }
    }

    fn update(&mut self, text: String) {
        self.source_text = text;
        self.symbol_index = SymbolIndex::new();
        self.diagnostics.clear();
    }
}

/// Resident compiler state for LSP server
pub struct CompilerState {
    documents: HashMap<String, DocumentState>,
    symbol_table: SymbolTable,
    #[allow(dead_code)]
    vm: VM,
}

impl CompilerState {
    /// Create new compiler state
    pub fn new() -> Self {
        let mut symbol_table = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbol_table);
        init_stdlib(&mut vm, &mut symbol_table);

        Self {
            documents: HashMap::new(),
            symbol_table,
            vm,
        }
    }

    /// Handle document open
    pub fn on_document_open(&mut self, uri: String, text: String) {
        let mut doc = DocumentState::new(uri.clone());
        doc.update(text);
        self.documents.insert(uri, doc);
    }

    /// Handle document change
    pub fn on_document_change(&mut self, uri: &str, text: String) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.update(text);
        }
    }

    /// Handle document close
    pub fn on_document_close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    /// Compile a document and generate diagnostics + symbol index
    pub fn compile_document(&mut self, uri: &str) -> bool {
        let Some(doc) = self.documents.get_mut(uri) else {
            return false;
        };

        // Clear previous state
        doc.diagnostics.clear();
        doc.symbol_index = SymbolIndex::new();

        // Analyze using the new pipeline
        let analyses = match analyze_all_new(&doc.source_text, &mut self.symbol_table) {
            Ok(results) => results,
            Err(e) => {
                // Analysis error - add as diagnostic
                doc.diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "E0001",
                    "syntax-error",
                    e,
                    None,
                ));
                return false;
            }
        };

        // Process each analysis result
        for analysis in &analyses {
            // Extract symbols and merge into document's symbol index
            let partial_index =
                extract_symbols_from_hir(&analysis.hir, &analysis.bindings, &self.symbol_table);
            doc.symbol_index.merge(partial_index);

            // Run HIR linter
            let mut linter = HirLinter::new(analysis.bindings.clone());
            linter.lint(&analysis.hir, &self.symbol_table);
            doc.diagnostics.extend(linter.diagnostics().iter().cloned());
        }

        true
    }

    /// Get document state
    pub fn get_document(&self, uri: &str) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    /// Get symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get all open documents
    pub fn documents(&self) -> impl Iterator<Item = &DocumentState> {
        self.documents.values()
    }
}

impl Default for CompilerState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_state_creation() {
        let state = CompilerState::new();
        assert_eq!(state.documents.len(), 0);
    }

    #[test]
    fn test_document_open_and_close() {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());
        assert_eq!(state.documents.len(), 1);

        state.on_document_close("file:///test.l");
        assert_eq!(state.documents.len(), 0);
    }

    #[test]
    fn test_document_change() {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());

        if let Some(doc) = state.documents.get("file:///test.l") {
            assert_eq!(doc.source_text, "(+ 1 2)");
        }

        state.on_document_change("file:///test.l", "(+ 3 4)".to_string());
        if let Some(doc) = state.documents.get("file:///test.l") {
            assert_eq!(doc.source_text, "(+ 3 4)");
        }
    }

    #[test]
    fn test_compile_simple_expression() {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "(+ 1 2)".to_string());
        let result = state.compile_document("file:///test.l");
        assert!(result);
    }
}
