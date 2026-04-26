//! Resident compiler state for LSP server
//!
//! Manages compilation state for open documents and provides
//! symbol index for IDE features.

use crate::context::set_symbol_table;
use crate::hir::{extract_symbols_from_hir, HirLinter};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use crate::{analyze_file, init_stdlib, register_primitives, VM};
use std::collections::HashMap;

/// Extract a `SourceLoc` from a reader/analyzer error string.
///
/// Reader and analyzer errors are formatted as `<file>:line:col: message`.
/// This parses the prefix to recover the structured location.
fn extract_location_from_error(msg: &str) -> Option<SourceLoc> {
    // Format: "<file>:line:col: message"
    let rest = msg.strip_prefix('<')?;
    let bracket_end = rest.find('>')?;
    let file = &rest[..bracket_end];
    // After "<file>" comes ":line:col: message"
    let tail = &rest[bracket_end + 1..]; // ":line:col: message"
    let tail = tail.strip_prefix(':')?; // "line:col: message"
    let (line_str, col_and_rest) = tail.split_once(':')?;
    let (col_str, _) = col_and_rest.split_once(": ")?;
    let line = line_str.parse::<usize>().ok()?;
    let col = col_str.parse::<usize>().ok()?;
    Some(SourceLoc::new(format!("<{}>", file), line, col))
}

/// Document state: source + diagnostics + symbol index
pub(crate) struct DocumentState {
    pub source_text: String,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentState {
    fn new() -> Self {
        Self {
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
    vm: VM,
}

impl CompilerState {
    /// Create new compiler state
    pub fn new() -> Self {
        let mut symbol_table = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbol_table);
        set_symbol_table(&mut symbol_table as *mut SymbolTable);
        init_stdlib(&mut vm, &mut symbol_table);

        Self {
            documents: HashMap::new(),
            symbol_table,
            vm,
        }
    }

    /// Handle document open
    pub fn on_document_open(&mut self, uri: String, text: String) {
        let mut doc = DocumentState::new();
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

        // Analyze using the file-as-letrec pipeline
        let analysis = match analyze_file(
            &doc.source_text,
            &mut self.symbol_table,
            &mut self.vm,
            "<lsp>",
        ) {
            Ok(result) => result,
            Err(e) => {
                // Analysis error - add as diagnostic
                let location = extract_location_from_error(&e);
                doc.diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "E0001",
                    "syntax-error",
                    e,
                    location,
                ));
                return false;
            }
        };

        // Extract symbols from the file-level HIR
        doc.symbol_index =
            extract_symbols_from_hir(&analysis.hir, &self.symbol_table, &analysis.arena);

        // Run HIR linter
        let mut linter = HirLinter::new();
        linter.lint(&analysis.hir, &self.symbol_table, &analysis.arena);
        doc.diagnostics.extend(linter.diagnostics().iter().cloned());

        true
    }

    /// Get document state
    pub(crate) fn get_document(&self, uri: &str) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    /// Get symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get the VM's documentation map
    pub fn docs(&self) -> &std::collections::HashMap<String, Doc> {
        &self.vm.docs
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

    #[test]
    fn test_extract_location_from_error() {
        // Standard reader error format
        let loc = extract_location_from_error("<lsp>:1:4: unterminated list");
        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.file, "<lsp>");
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 4);

        // Multi-digit line/col
        let loc = extract_location_from_error("<lsp>:12:34: some error");
        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.line, 12);
        assert_eq!(loc.col, 34);
    }

    #[test]
    fn test_extract_location_from_error_invalid() {
        // No angle brackets
        assert!(extract_location_from_error("something went wrong").is_none());
        // Missing colon-separated parts
        assert!(extract_location_from_error("<lsp>: message").is_none());
    }

    #[test]
    fn test_compile_syntax_error_has_location() {
        let mut state = CompilerState::new();
        state.on_document_open("file:///test.l".to_string(), "((((".to_string());
        state.compile_document("file:///test.l");
        let doc = state.get_document("file:///test.l").unwrap();
        assert!(!doc.diagnostics.is_empty());
        let diag = &doc.diagnostics[0];
        assert!(
            diag.location.is_some(),
            "parse error diagnostic should have a location"
        );
        let loc = diag.location.as_ref().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 4);
    }
}
