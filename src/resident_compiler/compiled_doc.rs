//! CompiledDocument type - holds all compilation results for a document

use crate::compiler::ast::ExprWithLoc;
use crate::compiler::bytecode::Bytecode;
use crate::compiler::linter::diagnostics::Diagnostic;
use crate::compiler::SymbolIndex;
use crate::error::LocationMap;
use std::time::SystemTime;

/// A fully compiled document with all metadata
#[derive(Clone)]
pub struct CompiledDocument {
    /// Original source text
    pub source_text: String,

    /// Abstract syntax tree with location information
    pub ast: ExprWithLoc,

    /// Compiled bytecode
    pub bytecode: Bytecode,

    /// Bytecode instruction index → source location mapping
    pub location_map: LocationMap,

    /// Extracted symbol information for IDE features
    pub symbols: SymbolIndex,

    /// Linter diagnostics (errors, warnings, info)
    pub diagnostics: Vec<Diagnostic>,

    /// When this document was compiled
    pub compiled_at: SystemTime,
}

impl CompiledDocument {
    /// Create a new compiled document
    pub fn new(
        source_text: String,
        ast: ExprWithLoc,
        bytecode: Bytecode,
        location_map: LocationMap,
        symbols: SymbolIndex,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            source_text,
            ast,
            bytecode,
            location_map,
            symbols,
            diagnostics,
            compiled_at: SystemTime::now(),
        }
    }

    /// Check if this document is still valid for a file (based on mtime)
    pub fn is_valid_for_file(&self, path: &str) -> bool {
        match std::fs::metadata(path) {
            Ok(metadata) => match metadata.modified() {
                Ok(mtime) => mtime <= self.compiled_at,
                Err(_) => false,
            },
            Err(_) => false,
        }
    }
}
