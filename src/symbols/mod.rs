//! Symbol index types for IDE features (hover, completion, go-to-definition)
//!
//! Pipeline-agnostic data types for symbol information. The extraction
//! functions that populate these types are pipeline-specific and live
//! in their respective modules.

use crate::reader::SourceLoc;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Kind of symbol for IDE classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// User-defined function
    Function,
    /// Variable or binding
    Variable,
    /// Built-in primitive
    Builtin,
    /// Macro
    Macro,
    /// Module
    Module,
}

impl SymbolKind {
    /// LSP completion kind string
    pub fn lsp_kind(&self) -> &'static str {
        match self {
            SymbolKind::Function => "Function",
            SymbolKind::Variable => "Variable",
            SymbolKind::Builtin => "Class",
            SymbolKind::Macro => "Keyword",
            SymbolKind::Module => "Module",
        }
    }
}

/// Information about a symbol definition
#[derive(Debug, Clone)]
pub struct SymbolDef {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub location: Option<SourceLoc>,
    pub arity: Option<usize>,
    pub documentation: Option<String>,
}

impl SymbolDef {
    pub fn new(id: SymbolId, name: String, kind: SymbolKind) -> Self {
        Self {
            id,
            name,
            kind,
            location: None,
            arity: None,
            documentation: None,
        }
    }

    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }

    pub fn with_arity(mut self, arity: usize) -> Self {
        self.arity = Some(arity);
        self
    }

    pub fn with_documentation(mut self, doc: String) -> Self {
        self.documentation = Some(doc);
        self
    }
}

/// Index of symbols extracted from compiled code
#[derive(Debug, Clone)]
pub struct SymbolIndex {
    /// All symbol definitions (both builtins and user-defined)
    pub definitions: HashMap<SymbolId, SymbolDef>,

    /// Symbol locations for go-to-definition
    pub symbol_locations: HashMap<SymbolId, SourceLoc>,

    /// Symbol usages for find-references
    pub symbol_usages: HashMap<SymbolId, Vec<SourceLoc>>,

    /// All available symbols for completion, grouped by kind
    pub available_symbols: Vec<(String, SymbolId, SymbolKind)>,
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            symbol_locations: HashMap::new(),
            symbol_usages: HashMap::new(),
            available_symbols: Vec::new(),
        }
    }

    /// Get documentation for a symbol
    pub fn get_documentation(&self, sym_id: SymbolId) -> Option<&str> {
        self.definitions
            .get(&sym_id)
            .and_then(|def| def.documentation.as_deref())
    }

    /// Get arity of a function
    pub fn get_arity(&self, sym_id: SymbolId) -> Option<usize> {
        self.definitions.get(&sym_id).and_then(|def| def.arity)
    }

    /// Get kind of symbol
    pub fn get_kind(&self, sym_id: SymbolId) -> Option<SymbolKind> {
        self.definitions.get(&sym_id).map(|def| def.kind)
    }

    /// Find symbol at a location (line, col)
    /// This would require source mapping which we'll implement in the LSP handler
    /// For now, this is a placeholder
    #[allow(unused)]
    pub fn find_symbol_at(&self, _line: usize, _col: usize) -> Option<SymbolId> {
        None
    }

    /// Merge another SymbolIndex into this one
    pub fn merge(&mut self, other: SymbolIndex) {
        self.definitions.extend(other.definitions);
        self.symbol_locations.extend(other.symbol_locations);
        for (sym_id, usages) in other.symbol_usages {
            self.symbol_usages.entry(sym_id).or_default().extend(usages);
        }
        self.available_symbols.extend(other.available_symbols);
    }
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_kind_lsp_kind() {
        assert_eq!(SymbolKind::Function.lsp_kind(), "Function");
        assert_eq!(SymbolKind::Variable.lsp_kind(), "Variable");
        assert_eq!(SymbolKind::Builtin.lsp_kind(), "Class");
    }

    #[test]
    fn test_symbol_def_builder() {
        let sym_id = SymbolId(1);
        let def = SymbolDef::new(sym_id, "test-var".to_string(), SymbolKind::Variable)
            .with_arity(2)
            .with_documentation("A test variable".to_string());

        assert_eq!(def.arity, Some(2));
        assert_eq!(def.documentation, Some("A test variable".to_string()));
    }

    #[test]
    fn test_symbol_index_creation() {
        let index = SymbolIndex::new();
        assert_eq!(index.definitions.len(), 0);
        assert_eq!(index.available_symbols.len(), 0);
    }
}
