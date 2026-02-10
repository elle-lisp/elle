//! Go-to-definition support for LSP
//!
//! Handles textDocument/definition requests by finding symbol
//! definitions using the symbol index and returning their locations.

use elle::compiler::symbol_index::SymbolIndex;
use elle::symbol::SymbolTable;
use serde_json::{json, Value};

/// Find definition location for a symbol at a given position
pub fn find_definition(
    line: u32,
    character: u32,
    symbol_index: &SymbolIndex,
    _symbol_table: &SymbolTable,
) -> Option<Value> {
    // LSP uses 0-based line numbers but SourceLoc uses 1-based
    let target_line = line as usize + 1;
    let target_col = character as usize + 1;

    // Look for symbols at this location (both usages and definitions)
    let mut closest_symbol = None;
    let mut closest_distance = usize::MAX;

    // Check symbol usages first
    for (sym_id, usages) in &symbol_index.symbol_usages {
        for usage_loc in usages {
            if usage_loc.line == target_line {
                let distance = (target_col as isize - usage_loc.col as isize).abs() as usize;
                if distance < closest_distance && distance <= 10 {
                    // Within 10 characters of the symbol
                    closest_symbol = Some(*sym_id);
                    closest_distance = distance;
                }
            }
        }
    }

    // Check symbol definitions
    for (sym_id, loc) in &symbol_index.symbol_locations {
        if loc.line == target_line {
            let distance = (target_col as isize - loc.col as isize).abs() as usize;
            if distance < closest_distance && distance <= 10 {
                closest_symbol = Some(*sym_id);
                closest_distance = distance;
            }
        }
    }

    // If we found a symbol, get its definition location
    closest_symbol.and_then(|sym_id| {
        // Look up the definition location
        if let Some(def_loc) = symbol_index.symbol_locations.get(&sym_id) {
            // Convert file URI: filename to file:///absolute/path
            // Phase 1: use the filename from the symbol index as-is
            // Phase 2: implement proper file resolution
            let uri = format!("file://{}", def_loc.file);

            // LSP uses 0-based line and character numbers
            Some(json!({
                "uri": uri,
                "range": {
                    "start": {
                        "line": def_loc.line.saturating_sub(1),
                        "character": def_loc.col.saturating_sub(1)
                    },
                    "end": {
                        "line": def_loc.line.saturating_sub(1),
                        "character": def_loc.col
                    }
                }
            }))
        } else if let Some(def) = symbol_index.definitions.get(&sym_id) {
            // Fallback to definition from symbol table
            if let Some(def_loc) = &def.location {
                let uri = format!("file://{}", def_loc.file);
                Some(json!({
                    "uri": uri,
                    "range": {
                        "start": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col.saturating_sub(1)
                        },
                        "end": {
                            "line": def_loc.line.saturating_sub(1),
                            "character": def_loc.col
                        }
                    }
                }))
            } else {
                None
            }
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_definition_returns_none_for_empty_index() {
        let index = SymbolIndex::new();
        let symbol_table = elle::SymbolTable::new();

        let definition = find_definition(0, 0, &index, &symbol_table);
        assert!(definition.is_none());
    }
}
