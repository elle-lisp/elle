//! Resident compiler state for LSP server
//!
//! Manages compilation state for open documents and provides
//! symbol index for IDE features.

use crate::hir::{extract_symbols_from_hir, HirLinter};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::pipeline::CompileCtx;
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use crate::{analyze_file, init_stdlib, register_primitives, VM};
use std::collections::HashMap;

/// Strip a `file://` scheme from a document URI to recover a filesystem path.
///
/// The path is handed to the analyzer as the source name so that every span —
/// and therefore every symbol-index location — carries the document's real
/// file. Reconstructing `file://` + path round-trips back to the request URI,
/// which is what makes rename/definition/references emit edits the client
/// accepts. Non-`file:` URIs (e.g. `untitled:`) pass through unchanged.
pub(crate) fn uri_to_source_name(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}

/// Extract a `SourceLoc` from a reader/analyzer error string.
///
/// Reader and analyzer errors are formatted as `file:line:col: message`, where
/// `file` is the source name — now a real path (e.g. `/home/u/foo.lisp`), not
/// just the old `<lsp>` sentinel. Parses from the right so a message containing
/// colons does not confuse the line/col fields.
fn extract_location_from_error(msg: &str) -> Option<SourceLoc> {
    let (prefix, _message) = msg.split_once(": ")?; // "file:line:col"
    let (file_and_line, col_str) = prefix.rsplit_once(':')?;
    let (file, line_str) = file_and_line.rsplit_once(':')?;
    let line = line_str.parse::<usize>().ok()?;
    let col = col_str.parse::<usize>().ok()?;
    Some(SourceLoc::new(file.to_string(), line, col))
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
    /// Boxed for a stable address — the VM holds a pointer to it
    /// (`VM::set_symbols`), so a macro transformer's `gensym` resolves names in
    /// THIS instance's own table.
    symbol_table: Box<SymbolTable>,
    vm: VM,
    /// This instance's per-instance compile context (macro expander, meta,
    /// projections). Boxed for a stable address (the VM holds a pointer to it).
    compile: Box<CompileCtx>,
}

impl CompilerState {
    /// Create new compiler state
    pub fn new() -> Self {
        let mut symbol_table = Box::new(SymbolTable::new());
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbol_table);
        // Point the VM at this instance's symbol table (stable boxed address);
        // macro expansion during analysis resolves names through it.
        vm.set_symbols(&mut *symbol_table as *mut SymbolTable);
        let mut compile = Box::new(CompileCtx::new());
        vm.set_compile_ctx(&mut *compile as *mut CompileCtx);
        init_stdlib(
            &mut vm,
            &mut symbol_table,
            &mut compile,
            &crate::compiler::stdlib_cache::StdlibCache::Process,
        );

        Self {
            documents: HashMap::new(),
            symbol_table,
            vm,
            compile,
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

        // Analyze using the file-as-letrec pipeline. The source name is the
        // document's real path (from its URI) so spans — hence every index
        // location — carry the actual file, which definition/references/rename
        // need to emit client-acceptable URIs.
        let source_name = uri_to_source_name(uri).to_string();
        let analysis = match analyze_file(
            &doc.source_text,
            &mut self.symbol_table,
            &mut self.vm,
            &mut self.compile,
            &source_name,
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

        // Extract symbols from the file-level HIR, then snap definition columns
        // onto the actual name tokens (the analyzer records them at the
        // initializer span). Usages are already accurate.
        doc.symbol_index =
            extract_symbols_from_hir(&analysis.hir, &self.symbol_table, &analysis.arena);
        crate::lsp::locate::snap_definition_locations(&mut doc.symbol_index, &doc.source_text);

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

    /// Iterate every open document's symbol index (for `workspace/symbol`).
    pub(crate) fn document_indices(&self) -> impl Iterator<Item = &SymbolIndex> {
        self.documents.values().map(|d| &d.symbol_index)
    }

    /// Get symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get the VM's documentation map
    pub fn docs(&self) -> &std::collections::HashMap<String, Doc> {
        &self.vm.docs
    }

    /// User-facing builtin/stdlib names for completion: every globally-bound
    /// callable (primitives, core.lisp, stdlib — including operators like `+`
    /// that are stdlib closures absent from `vm.docs`). Internal intrinsics
    /// (`%`-prefixed) and compiler gensyms (`__`-prefixed) are excluded.
    pub fn builtin_names(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        self.compile
            .global_function_ids()
            .filter_map(|id| self.symbol_table.name(id))
            .filter(|n| !n.starts_with('%') && !n.starts_with("__"))
            .filter(|n| seen.insert(*n))
            .collect()
    }
}

impl Default for CompilerState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
