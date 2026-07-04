//! Symbol index types for IDE features (hover, completion, go-to-definition)
//!
//! Pipeline-agnostic data types for symbol information. The extraction
//! functions that populate these types are pipeline-specific and live
//! in their respective modules.

use crate::reader::SourceLoc;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Per-occurrence identity for a symbol within one document's analysis.
///
/// Distinct *bindings* get distinct `DefId`s even when they share a name, so
/// two locals both spelled `x` in different scopes never collapse — the bug
/// that made rename/find-references over-apply when the index was keyed by the
/// interned name (`SymbolId`) alone.
///
/// Derived from the HIR `Binding` arena index at extraction time (see
/// `Binding::def_id`). It is opaque afterward — the arena is gone once
/// extraction finishes; only identity/equality is used. Kept pipeline-agnostic
/// (a bare `u32`, not a `Binding`) so this module stays free of `hir`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub u32);

impl DefId {
    pub fn new(raw: u32) -> Self {
        DefId(raw)
    }
}

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

    /// LSP `CompletionItemKind` numeric code (see the LSP spec enum).
    pub fn lsp_completion_kind(&self) -> u32 {
        match self {
            SymbolKind::Function => 3, // Function
            SymbolKind::Variable => 6, // Variable
            SymbolKind::Builtin => 3,  // Function (primitives are callables)
            SymbolKind::Macro => 14,   // Keyword
            SymbolKind::Module => 9,   // Module
        }
    }

    /// LSP `SymbolKind` numeric code for documentSymbol/workspaceSymbol.
    pub fn lsp_symbol_kind(&self) -> u32 {
        match self {
            SymbolKind::Function => 12, // Function
            SymbolKind::Variable => 13, // Variable
            SymbolKind::Builtin => 12,  // Function
            SymbolKind::Macro => 12,    // Function (no dedicated macro kind)
            SymbolKind::Module => 2,    // Module
        }
    }
}

/// Information about a symbol definition
#[derive(Debug, Clone)]
pub struct SymbolDef {
    /// Interned name id. Distinct from the index key (`DefId`): many bindings
    /// can share one `id` (same name) while each has its own `DefId`.
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

/// Index of symbols extracted from compiled code.
///
/// All maps are keyed by [`DefId`] (per-binding identity), so same-named
/// symbols in different scopes stay distinct. `SymbolDef` still carries the
/// interned [`SymbolId`] name for callers that group by name.
#[derive(Debug, Clone)]
pub struct SymbolIndex {
    /// Metadata for every binding encountered (both real in-file definitions
    /// and usage-only references such as primitives). A real definition has
    /// `location: Some`; a usage-only entry (e.g. a primitive) has
    /// `location: None`.
    pub definitions: HashMap<DefId, SymbolDef>,

    /// Definition site for each defined binding (go-to-definition target).
    pub symbol_locations: HashMap<DefId, SourceLoc>,

    /// Usage sites for each binding (find-references).
    pub symbol_usages: HashMap<DefId, Vec<SourceLoc>>,

    /// User-defined symbols for completion (name, id, kind). Excludes
    /// usage-only entries and synthetic compiler temporaries.
    pub available_symbols: Vec<(String, DefId, SymbolKind)>,
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
    pub fn get_documentation(&self, id: DefId) -> Option<&str> {
        self.definitions
            .get(&id)
            .and_then(|def| def.documentation.as_deref())
    }

    /// Get arity of a function
    pub fn get_arity(&self, id: DefId) -> Option<usize> {
        self.definitions.get(&id).and_then(|def| def.arity)
    }

    /// Get kind of symbol
    pub fn get_kind(&self, id: DefId) -> Option<SymbolKind> {
        self.definitions.get(&id).map(|def| def.kind)
    }

    /// Merge another SymbolIndex into this one.
    ///
    /// Caller must ensure the two indices use disjoint `DefId` spaces (each is
    /// built from its own arena starting at 0, so merging two raw indices
    /// would collide — there are currently no such callers).
    pub fn merge(&mut self, other: SymbolIndex) {
        self.definitions.extend(other.definitions);
        self.symbol_locations.extend(other.symbol_locations);
        for (id, usages) in other.symbol_usages {
            self.symbol_usages.entry(id).or_default().extend(usages);
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
mod tests;
