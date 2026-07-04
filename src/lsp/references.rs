//! Find references support for LSP

use crate::lsp::locate;
use crate::symbols::SymbolIndex;
use serde_json::Value;

/// Find all references to the symbol at a given position.
pub(crate) fn find_references(
    line: u32,
    character: u32,
    include_declaration: bool,
    symbol_index: &SymbolIndex,
) -> Vec<Value> {
    let Some(id) = locate::symbol_at(symbol_index, line, character) else {
        return Vec::new();
    };
    let name_len = symbol_index
        .definitions
        .get(&id)
        .map_or(0, |d| d.name.len());

    let mut references = Vec::new();
    if let Some(usages) = symbol_index.symbol_usages.get(&id) {
        for loc in usages {
            references.push(locate::location(loc, name_len));
        }
    }

    if include_declaration {
        let def_loc = symbol_index.symbol_locations.get(&id).or_else(|| {
            symbol_index
                .definitions
                .get(&id)
                .and_then(|d| d.location.as_ref())
        });
        if let Some(loc) = def_loc {
            references.push(locate::location(loc, name_len));
        }
    }

    references
}

#[cfg(test)]
mod tests;
