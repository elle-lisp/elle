//! Resident compiler state for LSP server
//!
//! Manages compilation state for open documents and provides
//! symbol index for IDE features.

use elle::compiler::converters::value_to_expr;
use elle::compiler::{ast::ExprWithLoc, extract_symbols, Linter, SymbolIndex};
use elle::reader::{Lexer, OwnedToken};
use elle::symbol::SymbolTable;
use elle::{init_stdlib, register_primitives, Reader, VM};
use std::collections::HashMap;

/// Document state: source + compiled expression + diagnostics
pub struct DocumentState {
    pub uri: String,
    pub source_text: String,
    pub compiled_expr: Option<ExprWithLoc>,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<elle::compiler::linter::diagnostics::Diagnostic>,
}

impl DocumentState {
    fn new(uri: String) -> Self {
        Self {
            uri,
            source_text: String::new(),
            compiled_expr: None,
            symbol_index: SymbolIndex::new(),
            diagnostics: Vec::new(),
        }
    }

    fn update(&mut self, text: String) {
        self.source_text = text;
        self.compiled_expr = None;
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

        // Parse the document
        let mut lexer = Lexer::new(&doc.source_text);
        let mut tokens = Vec::new();

        loop {
            match lexer.next_token() {
                Ok(Some(token)) => tokens.push(OwnedToken::from(token)),
                Ok(None) => break,
                Err(_e) => {
                    // Parse error - skip this document for now
                    return false;
                }
            }
        }

        let mut reader = Reader::new(tokens);
        let mut values = Vec::new();

        while let Some(result) = reader.try_read(&mut self.symbol_table) {
            match result {
                Ok(value) => values.push(value),
                Err(_e) => {
                    // Read error
                    return false;
                }
            }
        }

        // Convert values to exprs
        let mut exprs = Vec::new();
        for value in values {
            match value_to_expr(&value, &mut self.symbol_table) {
                Ok(expr) => {
                    exprs.push(ExprWithLoc::new(expr, None));
                }
                Err(_e) => {
                    // Conversion error
                    return false;
                }
            }
        }

        // Extract symbol index
        doc.symbol_index = extract_symbols(&exprs, &self.symbol_table);

        // Run linter
        let mut linter = Linter::new();
        for expr in &exprs {
            linter.lint_expr(expr, &self.symbol_table);
        }

        // Store diagnostics
        doc.diagnostics = linter.diagnostics().to_vec();

        // Store compiled expressions
        if !exprs.is_empty() {
            doc.compiled_expr = Some(exprs[exprs.len() - 1].clone());
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
