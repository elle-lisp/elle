// audited: 2026-10-13
//! The language server's resident state: the open documents, and the instance they compile against.
//!
//! src/lsp/README.md
//! docs/impl/image/boot.md
//!
//! Each document keeps its source, its diagnostics and its symbol index. Every
//! document compiles against one resident instance, which boots as `elle` does.

use crate::analyze_file;
use crate::hir::{extract_symbols_from_hir, HirLinter};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::runtime::{BootCaches, BootFrom, RuntimeCore};
use crate::symbol::SymbolTable;
use crate::symbols::SymbolIndex;
use std::collections::HashMap;

/// Strip a `file://` scheme from a document URI to recover a filesystem path.
///
/// The path is handed to the analyzer as the source name so that every span —
/// and therefore every symbol-index location — carries the document's real
/// file. Reconstructing `file://` + path round-trips back to the request URI,
/// which is what makes rename/definition/references emit edits the client
/// accepts. Non-`file:` URIs (for example `untitled:`) pass through unchanged.
pub(crate) fn uri_to_source_name(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}

/// Extract a `SourceLoc` from a reader/analyzer error string.
///
/// Reader and analyzer errors are formatted as `file:line:col: message`, where
/// `file` is the source name, which is the document's real path (for example
/// `/home/u/foo.lisp`). Parses from the right so a message containing colons
/// does not confuse the line/col fields.
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
    /// The resident instance every document compiles against. It boots as
    /// `elle` does, under the process policy (docs/impl/image/boot.md), and
    /// lives as long as the server.
    core: RuntimeCore,
}

impl CompilerState {
    /// Create new compiler state
    pub fn new() -> Self {
        let core = RuntimeCore::boot(
            crate::config::get().unicode_generation(),
            true,
            BootFrom::Caches(BootCaches::process()),
        )
        .core;
        Self {
            documents: HashMap::new(),
            core,
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
        let (vm, symbols, compile) = self.core.parts();
        let analysis = match analyze_file(&doc.source_text, symbols, vm, compile, &source_name) {
            Ok(result) => result,
            Err(e) => {
                // Analysis error - add as diagnostic
                let location = extract_location_from_error(&e);
                // `analyze_file` hands back a message, not an `ErrorKind`, so
                // the classification is fixed here rather than looked up. The
                // code still comes from the registry, so the LSP reports the
                // same one the lint CLI and `compile/diagnostics` do.
                let syntax_error = crate::lint::diagnostics::SYNTAX_ERROR;
                doc.diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    syntax_error.code,
                    syntax_error.rule,
                    e,
                    location,
                ));
                return false;
            }
        };

        // Extract symbols from the file-level HIR, then snap definition columns
        // onto the actual name tokens (the analyzer records them at the
        // initializer span). Usages are already accurate.
        doc.symbol_index = extract_symbols_from_hir(&analysis.hir, symbols, &analysis.arena);
        crate::lsp::locate::snap_definition_locations(&mut doc.symbol_index, &doc.source_text);

        // Run HIR linter
        let mut linter = HirLinter::new();
        linter.lint(&analysis.hir, symbols, &analysis.arena);
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
        self.core.parts_shared().1
    }

    /// Get the VM's documentation map
    pub fn docs(&self) -> &std::collections::HashMap<String, Doc> {
        &self.core.parts_shared().0.docs
    }

    /// User-facing builtin/stdlib names for completion: every globally-bound
    /// callable (primitives, core.lisp, stdlib — including operators like `+`
    /// that are stdlib closures absent from `vm.docs`). Internal intrinsics
    /// (`%`-prefixed) and compiler gensyms (`__`-prefixed) are excluded.
    pub fn builtin_names(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let (_vm, symbols, compile) = self.core.parts_shared();
        compile
            .global_function_ids()
            .filter_map(|id| symbols.name(id))
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
