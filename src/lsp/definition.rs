//! Go-to-definition support for LSP

use crate::lsp::locate;
use crate::symbols::SymbolIndex;
use serde_json::Value;

/// Find the definition location for the symbol at a given position.
pub(crate) fn find_definition(
    line: u32,
    character: u32,
    symbol_index: &SymbolIndex,
) -> Option<Value> {
    let id = locate::symbol_at(symbol_index, line, character)?;
    let name_len = symbol_index
        .definitions
        .get(&id)
        .map_or(0, |d| d.name.len());

    // Prefer the recorded definition site; fall back to the def's own location.
    let def_loc = symbol_index.symbol_locations.get(&id).or_else(|| {
        symbol_index
            .definitions
            .get(&id)
            .and_then(|d| d.location.as_ref())
    })?;

    Some(locate::location(def_loc, name_len))
}

#[cfg(test)]
mod tests;
