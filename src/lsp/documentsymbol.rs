//! Document- and workspace-symbol providers for LSP.
//!
//! Both answer "what symbols are defined here?" with flat `SymbolInformation`
//! entries. Only real in-file definitions appear: usage-only entries (e.g.
//! primitives, which carry no location) and synthetic compiler temporaries
//! (never recorded in the index) are excluded.

use crate::lsp::locate;
use crate::symbols::SymbolIndex;
use serde_json::{json, Value};

fn symbol_information(def: &crate::symbols::SymbolDef) -> Option<Value> {
    let loc = def.location.as_ref()?;
    Some(json!({
        "name": def.name,
        "kind": def.kind.lsp_symbol_kind(),
        "location": locate::location(loc, def.name.len()),
    }))
}

/// `textDocument/documentSymbol` → flat list of one document's definitions,
/// ordered by source line.
pub(crate) fn document_symbols(index: &SymbolIndex) -> Vec<Value> {
    let mut entries: Vec<(usize, Value)> = index
        .definitions
        .values()
        .filter_map(|def| {
            let line = def.location.as_ref()?.line;
            symbol_information(def).map(|v| (line, v))
        })
        .collect();
    entries.sort_by_key(|(line, _)| *line);
    entries.into_iter().map(|(_, v)| v).collect()
}

/// `workspace/symbol` → definitions across all open documents whose name
/// contains the (case-insensitive) query. An empty query returns everything.
pub(crate) fn workspace_symbols<'a>(
    indices: impl Iterator<Item = &'a SymbolIndex>,
    query: &str,
) -> Vec<Value> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for index in indices {
        for def in index.definitions.values() {
            if !needle.is_empty() && !def.name.to_lowercase().contains(&needle) {
                continue;
            }
            if let Some(v) = symbol_information(def) {
                out.push(v);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
